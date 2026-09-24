package com.teispace.teitunnel.control

import java.io.Closeable
import java.net.StandardProtocolFamily
import java.net.UnixDomainSocketAddress
import java.nio.ByteBuffer
import java.nio.channels.AsynchronousFileChannel
import java.nio.channels.SocketChannel
import java.nio.file.Path
import java.nio.file.StandardOpenOption
import java.util.concurrent.TimeUnit

/**
 * A byte stream to the app. Reading (one thread) and writing (others) happen at the
 * same time, so neither may block the other.
 */
interface Transport : Closeable {
    /** Reads into `buffer`; -1 at the end of the stream. */
    fun read(buffer: ByteBuffer): Int

    fun write(bytes: ByteArray)

    companion object {
        /** Connects to the app's Unix socket (Java 16+) or named pipe. */
        fun open(endpoint: Endpoint): Transport = if (endpoint.windows) PipeTransport(endpoint.address) else UnixTransport(endpoint.address)
    }
}

/**
 * A Unix domain socket (macOS, Linux). `SocketChannel` reads and writes on separate
 * locks, unlike `Channels.newInputStream`, which would block writes while reading.
 */
class UnixTransport(path: String) : Transport {
    private val channel: SocketChannel = SocketChannel.open(StandardProtocolFamily.UNIX)

    init {
        try {
            channel.connect(UnixDomainSocketAddress.of(Path.of(path)))
        } catch (_: Exception) {
            channel.close()
            throw ControlException.notRunning()
        }
    }

    override fun read(buffer: ByteBuffer): Int = channel.read(buffer)

    override fun write(bytes: ByteArray) {
        val buffer = ByteBuffer.wrap(bytes)
        synchronized(this) {
            while (buffer.hasRemaining()) channel.write(buffer)
        }
    }

    override fun close() = channel.close()
}

/**
 * A Windows named pipe, opened for overlapped I/O through `AsynchronousFileChannel`: a
 * pipe opened synchronously (`RandomAccessFile`) serialises I/O on its handle, so a
 * pending read would hold every write until the app sends something. The position is
 * ignored for pipes.
 */
class PipeTransport(name: String) : Transport {
    private val channel: AsynchronousFileChannel = try {
        AsynchronousFileChannel.open(Path.of(name), StandardOpenOption.READ, StandardOpenOption.WRITE)
    } catch (_: Exception) {
        throw ControlException.notRunning()
    }

    override fun read(buffer: ByteBuffer): Int = channel.read(buffer, 0).get()

    override fun write(bytes: ByteArray) {
        val buffer = ByteBuffer.wrap(bytes)
        synchronized(this) {
            while (buffer.hasRemaining()) channel.write(buffer, 0).get(30, TimeUnit.SECONDS)
        }
    }

    override fun close() = channel.close()
}
