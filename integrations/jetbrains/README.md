# Teitunnel for JetBrains IDEs

A tool window with your shares and routes (copy, open, open the inspector, stop), **Tools ▸
Teitunnel** (Share Port…, Run Doctor, Open Teitunnel) and a status bar widget, over the
running app's control connection. IntelliJ-based IDEs 2025.3 and later.

The protocol layer (`control/`) uses only the JDK: a Unix domain socket through
`SocketChannel` (Java 16+) on macOS and Linux, and the named pipe through
`AsynchronousFileChannel` on Windows (overlapped I/O, so a pending read never blocks a
write; a `RandomAccessFile` would). The Windows path is untested on Windows.

```sh
./gradlew test                 # protocol tests against a fake app on a Unix socket
./gradlew buildPlugin          # build/distributions/teitunnel-jetbrains-<version>.zip
./gradlew runIde               # a sandbox IDE with the plugin
./gradlew verifyPlugin         # JetBrains Plugin Verifier against recent IDEs
```

Publishing (maintainer): `CERTIFICATE_CHAIN`, `PRIVATE_KEY`, `PRIVATE_KEY_PASSWORD` and
`PUBLISH_TOKEN` in the environment, then `./gradlew signPlugin publishPlugin`.
