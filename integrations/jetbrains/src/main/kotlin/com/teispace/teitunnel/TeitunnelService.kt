package com.teispace.teitunnel

import com.intellij.ide.plugins.PluginManagerCore
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.extensions.PluginId
import com.intellij.util.concurrency.AppExecutorUtil
import com.teispace.teitunnel.control.ControlClient
import com.teispace.teitunnel.control.ControlException
import com.teispace.teitunnel.control.Route
import com.teispace.teitunnel.control.Share
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import kotlin.math.min

/**
 * The connection to the Teitunnel app for the whole IDE: connects in the background,
 * reconnects with backoff while the app isn't running or after it quits, subscribes to
 * events and keeps the shares and routes the tool window and status bar show.
 */
@Service(Service.Level.APP)
class TeitunnelService : Disposable {
    enum class State { CONNECTING, CONNECTED, DISCONNECTED }

    @Volatile var state: State = State.DISCONNECTED
        private set

    @Volatile var shares: List<Share> = emptyList()
        private set

    @Volatile var routes: List<Route> = emptyList()
        private set

    /** Why routes couldn't be listed (no account, several accounts…), if they couldn't. */
    @Volatile var routesProblem: String? = null
        private set

    /** Why the last connection attempt failed. */
    @Volatile var lastError: ControlException? = null
        private set

    private val listeners = CopyOnWriteArrayList<() -> Unit>()
    private val executor = AppExecutorUtil.createBoundedScheduledExecutorService("Teitunnel", 1)
    @Volatile private var client: ControlClient? = null
    @Volatile private var retry: ScheduledFuture<*>? = null
    @Volatile private var disposed = false
    private var attempt = 0

    private val version: String =
        PluginManagerCore.getPlugin(PluginId.getId("com.teispace.teitunnel"))?.version ?: "0.0.0"

    init {
        executor.execute { connect() }
    }

    /** Calls `listener` (on the UI thread) whenever state, shares or routes change. */
    fun addListener(parent: Disposable, listener: () -> Unit) {
        listeners.add(listener)
        com.intellij.openapi.util.Disposer.register(parent) { listeners.remove(listener) }
    }

    private fun changed() {
        ApplicationManager.getApplication().invokeLater { listeners.forEach { it() } }
    }

    /** Connects now (e.g. after opening the app) instead of waiting for the next retry. */
    fun reconnectSoon() {
        retry?.cancel(false)
        executor.execute { if (client?.isOpen != true) connect() else refresh() }
    }

    private fun connect() {
        if (disposed) return
        state = State.CONNECTING
        changed()
        try {
            val connected = ControlClient.connect(
                name = "jetbrains",
                version = version,
                onEvent = { event -> executor.execute { onEvent(event) } },
                onClosed = { executor.execute { onClosed() } },
            )
            client = connected
            connected.subscribe()
            attempt = 0
            lastError = null
            state = State.CONNECTED
            LOG.info("connected to Teitunnel ${connected.appVersion}")
            refresh()
        } catch (e: ControlException) {
            lastError = e
            state = State.DISCONNECTED
            changed()
            if (e.kind != ControlException.Kind.UNSUPPORTED) scheduleRetry()
        }
    }

    private fun onClosed() {
        // A connection refused during hello was already handled by connect().
        if (disposed || state != State.CONNECTED) return
        client = null
        state = State.DISCONNECTED
        shares = emptyList()
        routes = emptyList()
        changed()
        scheduleRetry()
    }

    private fun scheduleRetry() {
        if (disposed) return
        val base = min(30_000L, 500L shl min(attempt, 6))
        attempt++
        val delay = base / 2 + (Math.random() * base / 2).toLong()
        retry = executor.schedule({ connect() }, delay, TimeUnit.MILLISECONDS)
    }

    private fun onEvent(event: Map<String, Any?>) {
        when (event["type"]) {
            "sharesChanged" -> refreshShares()
            "routesChanged" -> refreshRoutes()
        }
    }

    /** Reads shares and routes again. */
    fun refresh() {
        refreshShares()
        refreshRoutes()
    }

    private fun refreshShares() {
        val current = client ?: return
        shares = try {
            current.shares()
        } catch (_: ControlException) {
            emptyList()
        }
        changed()
    }

    private fun refreshRoutes() {
        val current = client ?: return
        try {
            routes = current.routes()
            routesProblem = null
        } catch (e: ControlException) {
            routes = emptyList()
            routesProblem = e.message
        }
        changed()
    }

    /**
     * Runs `task` against the app off the UI thread; `done` gets its result on the UI
     * thread, `failed` the error (the app isn't running, the person said no…).
     */
    fun <T> run(task: (ControlClient) -> T, done: (T) -> Unit = {}, failed: (ControlException) -> Unit) {
        ApplicationManager.getApplication().executeOnPooledThread {
            val result = try {
                val current = client?.takeIf { it.isOpen } ?: throw (lastError ?: ControlException.notRunning())
                Result.success(task(current))
            } catch (e: ControlException) {
                Result.failure(e)
            }
            ApplicationManager.getApplication().invokeLater {
                result.fold(done) { failed(it as ControlException) }
            }
        }
    }

    override fun dispose() {
        disposed = true
        retry?.cancel(false)
        client?.close()
        executor.shutdownNow()
    }

    companion object {
        private val LOG = logger<TeitunnelService>()

        fun get(): TeitunnelService = service()
    }
}
