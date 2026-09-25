package com.teispace.teitunnel

import com.teispace.teitunnel.control.ControlClient
import com.teispace.teitunnel.control.ControlException
import com.teispace.teitunnel.control.Endpoint
import com.teispace.teitunnel.control.Json
import com.teispace.teitunnel.control.Share
import com.teispace.teitunnel.control.parseOrigin
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Assume.assumeFalse
import org.junit.Test
import java.net.StandardProtocolFamily
import java.net.UnixDomainSocketAddress
import java.nio.ByteBuffer
import java.nio.channels.ServerSocketChannel
import java.nio.channels.SocketChannel
import java.nio.file.Files
import java.nio.file.Path
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** The protocol layer, against a fake app on a real Unix socket (macOS, Linux). */
class ControlTest {
    private val windows = System.getProperty("os.name").lowercase().contains("win")
    private var dir: Path? = null
    private var server: ServerSocketChannel? = null

    @After
    fun cleanUp() {
        server?.close()
        dir?.toFile()?.deleteRecursively()
    }

    @Test
    fun `reads and writes JSON`() {
        val value = Json.parse("""{"a":[1,2.5,"x\né"],"b":null,"c":true,"d":{}}""") as Map<*, *>
        assertEquals(listOf(1L, 2.5, "x\né"), value["a"])
        assertNull(value["b"])
        assertEquals("""{"a":[1,2.5,"x\né"],"b":null,"c":true,"d":{}}""", Json.write(value))
        assertEquals("""{"s":"q\"b\\"}""", Json.write(mapOf("s" to "q\"b\\")))
        try {
            Json.parse("{\"a\":")
            fail("incomplete JSON")
        } catch (_: IllegalArgumentException) {
        }
    }

    @Test
    fun `finds the data folder on each platform`() {
        assertEquals(
            Path.of("/Users/a/Library/Application Support/com.teispace.teitunnel"),
            Endpoint.dataDir("Mac OS X", emptyMap(), "/Users/a"),
        )
        assertEquals(Path.of("/home/a/.local/share/com.teispace.teitunnel"), Endpoint.dataDir("Linux", emptyMap(), "/home/a"))
        assertEquals(Path.of("/x/com.teispace.teitunnel"), Endpoint.dataDir("Linux", mapOf("XDG_DATA_HOME" to "/x"), "/home/a"))
        assertEquals(Path.of("/tmp/tt"), Endpoint.dataDir("Linux", mapOf("TEITUNNEL_DATA_DIR" to "/tmp/tt"), "/home/a"))
    }

    @Test
    fun `accepts ports as typed`() {
        assertEquals("3000", parseOrigin(" 3000 "))
        assertEquals("5173", parseOrigin(":5173"))
        assertEquals("localhost:8080", parseOrigin("localhost:8080"))
        assertNull(parseOrigin("99999"))
        assertNull(parseOrigin("my app"))
    }

    @Test
    fun `explains a missing app`() {
        val empty = Files.createTempDirectory("tt-")
        dir = empty
        try {
            Endpoint.resolve(empty, windows = false)
            fail("no token")
        } catch (e: ControlException) {
            assertEquals(ControlException.Kind.NOT_INSTALLED, e.kind)
        }
        Files.createDirectories(empty.resolve("control"))
        Files.writeString(empty.resolve("control/token"), "a".repeat(64))
        try {
            ControlClient.connect("jetbrains", "test", Endpoint.resolve(empty, windows = false))
            fail("nothing listens")
        } catch (e: ControlException) {
            assertTrue(e.appUnavailable)
            assertFalse(e.message!!.contains("a".repeat(64)))
        }
    }

    @Test
    fun `says hello, calls methods, receives events, and reports a declined change quietly`() {
        assumeFalse("Unix sockets", windows)
        val data = Files.createTempDirectory("tt-")
        dir = data
        Files.createDirectories(data.resolve("control"))
        val token = "0123456789abcdef".repeat(4)
        Files.writeString(data.resolve("control/token"), token)
        val listener = ServerSocketChannel.open(StandardProtocolFamily.UNIX)
        listener.bind(UnixDomainSocketAddress.of(data.resolve("control/sock")))
        server = listener
        val received = CopyOnWriteArrayList<Map<*, *>>()
        Thread {
            val socket = listener.accept()
            serve(socket, token, received)
        }.apply { isDaemon = true }.start()

        val events = CountDownLatch(1)
        val eventTypes = CopyOnWriteArrayList<Any?>()
        val client = ControlClient.connect(
            "jetbrains",
            "0.1.0",
            Endpoint.resolve(data, windows = false),
            onEvent = {
                eventTypes.add(it["type"])
                events.countDown()
            },
        )
        assertEquals("9.9.9", client.appVersion)
        val shares = client.shares()
        assertEquals(listOf("a.trycloudflare.com"), shares.map(Share::label))
        assertTrue(shares[0].live)
        client.subscribe()
        assertTrue(events.await(5, TimeUnit.SECONDS))
        // Every typed event is passed on; the service acts on the ones it knows.
        assertEquals("sharesChanged", eventTypes.first())
        try {
            client.stopShare("qs-1")
            fail("declined")
        } catch (e: ControlException) {
            assertTrue(e.declined)
        }
        client.close()
        val hello = received.first()
        assertEquals("hello", hello["method"])
        assertEquals(mapOf("protocol" to 1L, "token" to token, "client" to mapOf("name" to "jetbrains", "version" to "0.1.0")), hello["params"])
        assertEquals(listOf("hello", "shares.list", "events.subscribe", "shares.stop"), received.map { it["method"] })
    }

    /** A fake app: the same framing and hello rules as `crates/control`. */
    private fun serve(socket: SocketChannel, token: String, received: MutableList<Map<*, *>>) {
        val buffer = ByteBuffer.allocate(4096)
        val line = StringBuilder()
        fun send(message: Map<String, Any?>) {
            val bytes = (Json.write(message) + "\n").toByteArray()
            val out = ByteBuffer.wrap(bytes)
            while (out.hasRemaining()) socket.write(out)
        }
        while (socket.read(buffer.clear()) >= 0) {
            buffer.flip()
            while (buffer.hasRemaining()) {
                val c = buffer.get().toInt().toChar()
                if (c != '\n') {
                    line.append(c)
                    continue
                }
                val request = Json.parse(line.toString()) as Map<*, *>
                line.clear()
                received.add(request)
                val id = request["id"]
                when (request["method"]) {
                    "hello" -> {
                        val params = request["params"] as Map<*, *>
                        if (params["token"] != token) {
                            send(mapOf("jsonrpc" to "2.0", "id" to id, "error" to mapOf("code" to -32001, "message" to "Wrong token.")))
                            socket.close()
                            return
                        }
                        send(mapOf("jsonrpc" to "2.0", "id" to id, "result" to mapOf("protocol" to 1, "app" to mapOf("name" to "Teitunnel", "version" to "9.9.9"), "approved" to false)))
                    }
                    "shares.list" -> send(
                        mapOf(
                            "jsonrpc" to "2.0",
                            "id" to id,
                            "result" to listOf(
                                mapOf("id" to "qs-1", "kind" to "quick", "url" to "https://a.trycloudflare.com", "origin" to "http://localhost:3000", "status" to "live", "requests" to 2, "startedAt" to 1),
                            ),
                        ),
                    )
                    "events.subscribe" -> {
                        send(mapOf("jsonrpc" to "2.0", "id" to id, "result" to mapOf("events" to listOf("sharesChanged"))))
                        send(mapOf("jsonrpc" to "2.0", "method" to "event", "params" to mapOf("type" to "sharesChanged", "id" to "qs-1")))
                        send(mapOf("jsonrpc" to "2.0", "method" to "event", "params" to mapOf("type" to "somethingNew")))
                    }
                    "shares.stop" -> send(mapOf("jsonrpc" to "2.0", "id" to id, "error" to mapOf("code" to -32003, "message" to "The change wasn't allowed in Teitunnel.")))
                    else -> send(mapOf("jsonrpc" to "2.0", "id" to id, "error" to mapOf("code" to -32601, "message" to "No method.")))
                }
            }
        }
    }
}
