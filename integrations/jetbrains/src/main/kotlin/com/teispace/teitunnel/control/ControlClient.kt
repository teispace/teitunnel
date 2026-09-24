package com.teispace.teitunnel.control

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * One connection to the running Teitunnel app: newline-delimited JSON-RPC 2.0, `hello`
 * with the install's token first. Blocking calls; a daemon thread reads answers and
 * events. Reconnecting is the caller's job (see `TeitunnelService`).
 */
class ControlClient private constructor(
    private val transport: Transport,
    private val onEvent: (Map<String, Any?>) -> Unit,
    private val onClosed: () -> Unit,
) : AutoCloseable {
    private val nextId = AtomicLong(1)
    private val pending = ConcurrentHashMap<Long, CompletableFuture<Any?>>()
    private val closed = AtomicBoolean(false)

    /** The app's answer to `hello`. */
    lateinit var hello: Map<String, Any?>
        private set

    val appVersion: String
        get() = ((hello["app"] as? Map<*, *>)?.get("version") as? String) ?: "?"

    val isOpen: Boolean get() = !closed.get()

    companion object {
        const val READ_TIMEOUT_SECONDS = 60L
        /** A change waits for the person's answer in the app (the app allows 180 s). */
        const val CHANGE_TIMEOUT_SECONDS = 190L
        private val CHANGES = setOf("shares.start", "shares.stop", "routes.apply")

        /**
         * Connects and says hello as `name`/`version`.
         *
         * @throws ControlException the app isn't set up or running, or refused us.
         */
        fun connect(
            name: String,
            version: String,
            endpoint: Endpoint = Endpoint.resolve(),
            onEvent: (Map<String, Any?>) -> Unit = {},
            onClosed: () -> Unit = {},
        ): ControlClient {
            val client = ControlClient(Transport.open(endpoint), onEvent, onClosed)
            client.start()
            try {
                @Suppress("UNCHECKED_CAST")
                client.hello = client.call(
                    "hello",
                    mapOf("protocol" to Protocol.VERSION, "token" to endpoint.token, "client" to mapOf("name" to name, "version" to version)),
                    5,
                ) as? Map<String, Any?> ?: emptyMap()
            } catch (e: ControlException) {
                client.close()
                throw e
            }
            return client
        }
    }

    private fun start() {
        val reader = Thread({ readLoop() }, "Teitunnel control reader")
        reader.isDaemon = true
        reader.start()
    }

    private fun readLoop() {
        val buffer = ByteBuffer.allocate(64 * 1024)
        val line = ByteArrayOutputStream()
        try {
            while (!closed.get()) {
                buffer.clear()
                val n = transport.read(buffer)
                if (n < 0) break
                buffer.flip()
                while (buffer.hasRemaining()) {
                    val b = buffer.get()
                    if (b == '\n'.code.toByte()) {
                        val text = line.toString(Charsets.UTF_8).trimEnd('\r')
                        line.reset()
                        if (text.isNotBlank()) receive(text)
                    } else {
                        if (line.size() >= Protocol.MAX_MESSAGE) throw IllegalStateException("message too large")
                        line.write(b.toInt())
                    }
                }
            }
        } catch (_: Exception) {
            // Closed, or the app went away.
        } finally {
            close()
        }
    }

    private fun receive(text: String) {
        val message = try {
            Json.parse(text) as? Map<*, *>
        } catch (_: IllegalArgumentException) {
            null
        } ?: return
        if (message["method"] == "event" && !message.containsKey("id")) {
            @Suppress("UNCHECKED_CAST")
            (message["params"] as? Map<String, Any?>)?.takeIf { it["type"] is String }?.let(onEvent)
            return
        }
        val id = (message["id"] as? Number)?.toLong() ?: return
        val future = pending.remove(id) ?: return
        val error = message["error"] as? Map<*, *>
        if (error != null) future.completeExceptionally(ControlException.fromRpc(error)) else future.complete(message["result"])
    }

    /**
     * Sends a request and waits for its answer.
     *
     * @throws ControlException an error from the app (`declined` when the person said no),
     * a timeout, or a closed connection.
     */
    fun request(method: String, params: Any? = null): Any? =
        call(method, params, if (method in CHANGES) CHANGE_TIMEOUT_SECONDS else READ_TIMEOUT_SECONDS)

    private fun call(method: String, params: Any?, timeoutSeconds: Long): Any? {
        if (closed.get()) throw ControlException.disconnected()
        val id = nextId.getAndIncrement()
        val future = CompletableFuture<Any?>()
        pending[id] = future
        val message = linkedMapOf<String, Any?>("jsonrpc" to "2.0", "id" to id, "method" to method)
        if (params != null) message["params"] = params
        try {
            transport.write((Json.write(message) + "\n").toByteArray(Charsets.UTF_8))
            return future.get(timeoutSeconds, TimeUnit.SECONDS)
        } catch (e: ExecutionException) {
            throw e.cause as? ControlException ?: ControlException.disconnected()
        } catch (_: TimeoutException) {
            throw ControlException(ControlException.Kind.TIMEOUT, "Teitunnel didn't answer in time.")
        } catch (e: ControlException) {
            throw e
        } catch (_: Exception) {
            throw ControlException.disconnected()
        } finally {
            pending.remove(id)
        }
    }

    fun shares(): List<Share> = (request("shares.list") as? List<*>).orEmpty().mapNotNull(Share::from)

    fun startShare(origin: String): Share =
        Share.from(request("shares.start", mapOf("origin" to origin))) ?: throw ControlException(ControlException.Kind.RPC, "Teitunnel sent an unexpected answer.")

    fun stopShare(id: String) {
        request("shares.stop", mapOf("id" to id))
    }

    fun routes(): List<Route> = ((request("routes.list") as? Map<*, *>)?.get("routes") as? List<*>).orEmpty().mapNotNull(Route::from)

    fun open(view: Map<String, String>) {
        request("open", view)
    }

    fun doctor(): List<DoctorIssue> = (request("doctor.run") as? List<*>).orEmpty().mapNotNull(DoctorIssue::from)

    fun subscribe(events: List<String>? = null) {
        request("events.subscribe", events?.let { mapOf("events" to it) })
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        try {
            transport.close()
        } catch (_: Exception) {
        }
        for (future in pending.values) future.completeExceptionally(ControlException.disconnected())
        pending.clear()
        onClosed()
    }
}
