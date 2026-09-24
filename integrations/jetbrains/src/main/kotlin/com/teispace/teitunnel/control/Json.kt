package com.teispace.teitunnel.control

/**
 * A small JSON reader and writer for the control connection's messages, so the protocol
 * layer needs nothing beyond the JDK (and is tested without the IDE). Values are
 * `Map<String, Any?>`, `List<Any?>`, `String`, `Double`/`Long`, `Boolean` or `null`.
 */
object Json {
    fun parse(text: String): Any? {
        val parser = Parser(text)
        val value = parser.value()
        parser.skipSpace()
        if (!parser.done()) throw IllegalArgumentException("trailing characters in JSON")
        return value
    }

    fun write(value: Any?): String = StringBuilder().also { write(it, value) }.toString()

    private fun write(out: StringBuilder, value: Any?) {
        when (value) {
            null -> out.append("null")
            is String -> string(out, value)
            is Boolean -> out.append(value)
            is Int, is Long, is Short, is Byte -> out.append(value)
            is Double -> if (value % 1.0 == 0.0 && kotlin.math.abs(value) < 1e15) out.append(value.toLong()) else out.append(value)
            is Number -> out.append(value.toString())
            is Map<*, *> -> {
                out.append('{')
                var first = true
                for ((key, item) in value) {
                    if (!first) out.append(',')
                    first = false
                    string(out, key.toString())
                    out.append(':')
                    write(out, item)
                }
                out.append('}')
            }
            is Iterable<*> -> {
                out.append('[')
                value.forEachIndexed { index, item ->
                    if (index > 0) out.append(',')
                    write(out, item)
                }
                out.append(']')
            }
            else -> string(out, value.toString())
        }
    }

    private fun string(out: StringBuilder, text: String) {
        out.append('"')
        for (c in text) {
            when (c) {
                '"' -> out.append("\\\"")
                '\\' -> out.append("\\\\")
                '\n' -> out.append("\\n")
                '\r' -> out.append("\\r")
                '\t' -> out.append("\\t")
                else -> if (c < ' ') out.append(String.format("\\u%04x", c.code)) else out.append(c)
            }
        }
        out.append('"')
    }

    private class Parser(private val text: String) {
        private var at = 0

        fun done() = at >= text.length

        fun skipSpace() {
            while (at < text.length && text[at].isWhitespace()) at++
        }

        fun value(): Any? {
            skipSpace()
            if (done()) throw IllegalArgumentException("unexpected end of JSON")
            return when (val c = text[at]) {
                '{' -> obj()
                '[' -> array()
                '"' -> string()
                't' -> literal("true", true)
                'f' -> literal("false", false)
                'n' -> literal("null", null)
                else -> if (c == '-' || c.isDigit()) number() else throw IllegalArgumentException("unexpected '$c' in JSON")
            }
        }

        private fun literal(word: String, value: Any?): Any? {
            if (!text.startsWith(word, at)) throw IllegalArgumentException("bad literal in JSON")
            at += word.length
            return value
        }

        private fun number(): Any {
            val start = at
            if (text[at] == '-') at++
            while (at < text.length && (text[at].isDigit() || text[at] in ".eE+-")) at++
            val raw = text.substring(start, at)
            return raw.toLongOrNull() ?: raw.toDouble()
        }

        private fun string(): String {
            at++ // opening quote
            val out = StringBuilder()
            while (true) {
                if (done()) throw IllegalArgumentException("unterminated string in JSON")
                when (val c = text[at++]) {
                    '"' -> return out.toString()
                    '\\' -> {
                        when (val e = text[at++]) {
                            '"', '\\', '/' -> out.append(e)
                            'b' -> out.append('\b')
                            'f' -> out.append('\u000c')
                            'n' -> out.append('\n')
                            'r' -> out.append('\r')
                            't' -> out.append('\t')
                            'u' -> {
                                out.append(text.substring(at, at + 4).toInt(16).toChar())
                                at += 4
                            }
                            else -> throw IllegalArgumentException("bad escape in JSON")
                        }
                    }
                    else -> out.append(c)
                }
            }
        }

        private fun array(): List<Any?> {
            at++
            val items = mutableListOf<Any?>()
            skipSpace()
            if (text.getOrNull(at) == ']') {
                at++
                return items
            }
            while (true) {
                items.add(value())
                skipSpace()
                when (text.getOrNull(at++)) {
                    ',' -> continue
                    ']' -> return items
                    else -> throw IllegalArgumentException("expected , or ] in JSON")
                }
            }
        }

        private fun obj(): Map<String, Any?> {
            at++
            val map = LinkedHashMap<String, Any?>()
            skipSpace()
            if (text.getOrNull(at) == '}') {
                at++
                return map
            }
            while (true) {
                skipSpace()
                if (text.getOrNull(at) != '"') throw IllegalArgumentException("expected a key in JSON")
                val key = string()
                skipSpace()
                if (text.getOrNull(at++) != ':') throw IllegalArgumentException("expected : in JSON")
                map[key] = value()
                skipSpace()
                when (text.getOrNull(at++)) {
                    ',' -> continue
                    '}' -> return map
                    else -> throw IllegalArgumentException("expected , or } in JSON")
                }
            }
        }
    }
}
