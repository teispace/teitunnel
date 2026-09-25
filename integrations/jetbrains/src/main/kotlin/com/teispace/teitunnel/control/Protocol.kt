package com.teispace.teitunnel.control

/**
 * The control connection's types (protocol 1, `crates/control/src/protocol.rs`), read
 * leniently from JSON: unknown fields and event types are ignored.
 */
object Protocol {
    const val VERSION = 1
    const val MAX_MESSAGE = 1024 * 1024
    const val OPEN_APP_URL = "teitunnel://open"

    const val UNAUTHORIZED = -32001L
    const val DECLINED = -32003L
    const val DISABLED = -32006L
    const val UNSUPPORTED_PROTOCOL = -32010L
}

data class Share(
    val id: String,
    val kind: String,
    val url: String?,
    val origin: String,
    val status: String,
    val requests: Long?,
    val error: String?,
) {
    val live: Boolean get() = status == "live" && url != null

    /** The address without its scheme, else what it shares. */
    val label: String get() = shortUrl(url ?: origin)

    companion object {
        fun from(value: Any?): Share? {
            val map = value as? Map<*, *> ?: return null
            return Share(
                id = map["id"] as? String ?: return null,
                kind = map["kind"] as? String ?: "quick",
                url = map["url"] as? String,
                origin = map["origin"] as? String ?: "",
                status = map["status"] as? String ?: "unknown",
                requests = (map["requests"] as? Number)?.toLong(),
                error = map["error"] as? String,
            )
        }
    }
}

data class Route(
    val hostname: String,
    val path: String?,
    val origin: String,
    val status: String,
    val statusText: String,
) {
    val url: String get() = "https://$hostname${path ?: ""}"

    companion object {
        fun from(value: Any?): Route? {
            val map = value as? Map<*, *> ?: return null
            return Route(
                hostname = map["hostname"] as? String ?: return null,
                path = map["path"] as? String,
                origin = map["origin"] as? String ?: "",
                status = map["status"] as? String ?: "",
                statusText = map["statusText"] as? String ?: "",
            )
        }
    }
}

data class DoctorIssue(val severity: String, val title: String, val subject: String, val detail: String) {
    companion object {
        fun from(value: Any?): DoctorIssue? {
            val map = value as? Map<*, *> ?: return null
            return DoctorIssue(
                severity = map["severity"] as? String ?: "info",
                title = map["title"] as? String ?: return null,
                subject = map["subject"] as? String ?: "",
                detail = map["detail"] as? String ?: "",
            )
        }
    }
}

/** A URL without its scheme or trailing slash. */
fun shortUrl(url: String): String = url.replace(Regex("^[a-zA-Z]+://"), "").trimEnd('/')

/** A port as typed: `3000`, `:3000`, `localhost:3000` or a local URL; null if it isn't one. */
fun parseOrigin(input: String): String? {
    val text = input.trim()
    Regex("^:?(\\d{1,5})$").find(text)?.let { match ->
        val port = match.groupValues[1].toInt()
        return if (port in 1..65535) port.toString() else null
    }
    return if (Regex("^(https?://)?[\\w.-]+:\\d{1,5}/?$").matches(text)) text else null
}

/** What went wrong on the control connection. */
class ControlException(
    val kind: Kind,
    message: String,
    val code: Long? = null,
) : Exception(message) {
    enum class Kind { NOT_INSTALLED, NOT_RUNNING, DISABLED, UNAUTHORIZED, UNSUPPORTED, DECLINED, TIMEOUT, DISCONNECTED, RPC }

    /** The person said no in Teitunnel: nothing to report. */
    val declined: Boolean get() = kind == Kind.DECLINED

    /** Opening Teitunnel would help. */
    val appUnavailable: Boolean get() = kind == Kind.NOT_RUNNING || kind == Kind.NOT_INSTALLED

    companion object {
        fun notInstalled() = ControlException(
            Kind.NOT_INSTALLED,
            "Teitunnel isn't set up on this computer. Install it from teitunnel.teispace.com and open it once.",
        )

        fun notRunning() = ControlException(Kind.NOT_RUNNING, "Teitunnel isn't running. Open Teitunnel and try again.")

        fun disconnected() = ControlException(Kind.DISCONNECTED, "The connection to Teitunnel closed.")

        fun fromRpc(error: Map<*, *>): ControlException {
            val code = (error["code"] as? Number)?.toLong()
            val message = error["message"] as? String ?: "Teitunnel couldn't do that."
            return when (code) {
                Protocol.DECLINED -> ControlException(Kind.DECLINED, "The change wasn't allowed in Teitunnel.", code)
                Protocol.UNAUTHORIZED -> ControlException(Kind.UNAUTHORIZED, "Teitunnel didn't accept this plugin's token. Try again.", code)
                Protocol.DISABLED -> ControlException(
                    Kind.DISABLED,
                    "Teitunnel's connection for extensions is turned off. Turn on Settings ▸ Integrations ▸ Allow connections.",
                    code,
                )
                Protocol.UNSUPPORTED_PROTOCOL -> ControlException(
                    Kind.UNSUPPORTED,
                    "This plugin and your Teitunnel app don't speak the same version. Update both.",
                    code,
                )
                else -> ControlException(Kind.RPC, message, code)
            }
        }
    }
}
