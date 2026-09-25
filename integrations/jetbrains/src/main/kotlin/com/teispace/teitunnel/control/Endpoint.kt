package com.teispace.teitunnel.control

import java.nio.file.Files
import java.nio.file.Path

/**
 * Where the running app listens: `<data>/control/token`, and `sock` (a Unix socket,
 * macOS and Linux) or `pipe` (a file naming a Windows named pipe).
 */
data class Endpoint(
    /** The socket's path, or the pipe's name (`\\.\pipe\teitunnel-control-…`). */
    val address: String,
    /** The token for `hello` (never logged or shown). */
    val token: String,
    val windows: Boolean,
) {
    override fun toString(): String = "Endpoint($address)"

    companion object {
        const val IDENTIFIER = "com.teispace.teitunnel"
        private const val PIPE_PREFIX = "\\\\.\\pipe\\teitunnel-control-"

        /**
         * The app's data folder, as the app and CLI find it: `TEITUNNEL_DATA_DIR`, else
         * `~/Library/Application Support/…` (macOS), `%APPDATA%\…` (Windows),
         * `$XDG_DATA_HOME/…` or `~/.local/share/…` (Linux).
         */
        fun dataDir(
            os: String = System.getProperty("os.name"),
            env: Map<String, String> = System.getenv(),
            home: String = System.getProperty("user.home"),
        ): Path {
            env["TEITUNNEL_DATA_DIR"]?.takeIf { it.isNotBlank() }?.let { return Path.of(it) }
            val name = os.lowercase()
            return when {
                name.contains("mac") -> Path.of(home, "Library", "Application Support", IDENTIFIER)
                name.contains("win") -> Path.of(env["APPDATA"] ?: Path.of(home, "AppData", "Roaming").toString(), IDENTIFIER)
                else -> {
                    val xdg = env["XDG_DATA_HOME"]?.takeIf { it.startsWith("/") }
                    Path.of(xdg ?: Path.of(home, ".local", "share").toString(), IDENTIFIER)
                }
            }
        }

        /** Reads the token and finds the socket or pipe (read fresh each time). */
        fun resolve(
            data: Path = dataDir(),
            windows: Boolean = System.getProperty("os.name").lowercase().contains("win"),
        ): Endpoint {
            val dir = data.resolve("control")
            val token = try {
                Files.readString(dir.resolve("token")).trim()
            } catch (_: Exception) {
                throw ControlException.notInstalled()
            }
            if (!Regex("^[0-9a-fA-F]{64}$").matches(token)) throw ControlException.notInstalled()
            if (!windows) return Endpoint(dir.resolve("sock").toString(), token.lowercase(), false)
            val pipe = try {
                Files.readString(dir.resolve("pipe")).trim()
            } catch (_: Exception) {
                throw ControlException.notRunning()
            }
            if (!pipe.startsWith(PIPE_PREFIX) || pipe.length != PIPE_PREFIX.length + 32) throw ControlException.notRunning()
            return Endpoint(pipe, token.lowercase(), true)
        }
    }
}
