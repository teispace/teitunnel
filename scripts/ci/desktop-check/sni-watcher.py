"""A minimal StatusNotifierWatcher for CI: the D-Bus service a Linux desktop's panel runs
for tray icons (GNOME with the AppIndicator extension, KDE, XFCE…). It accepts
registrations and appends each item's service name to the file given as the argument, so
the desktop check can confirm Teitunnel's tray icon registers without a real panel.

    python3 sni-watcher.py registered.txt
"""

import sys

from gi.repository import Gio, GLib

INTERFACE = """
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
    <method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
    <signal name="StatusNotifierHostRegistered"/>
  </interface>
</node>
"""

out = sys.argv[1]
items: list[str] = []


def on_call(connection, sender, path, interface, method, params, invocation):
    if method == "RegisterStatusNotifierItem":
        (service,) = params.unpack()
        # Items may register an object path; the sender then names the service.
        item = f"{sender}{service}" if service.startswith("/") else service
        items.append(item)
        with open(out, "a", encoding="utf-8") as f:
            f.write(item + "\n")
        connection.emit_signal(
            None, path, interface, "StatusNotifierItemRegistered", GLib.Variant("(s)", (item,))
        )
    invocation.return_value(None)


def on_get(connection, sender, path, interface, prop):
    return {
        "RegisteredStatusNotifierItems": GLib.Variant("as", items),
        "IsStatusNotifierHostRegistered": GLib.Variant("b", True),
        "ProtocolVersion": GLib.Variant("i", 0),
    }[prop]


def on_bus(connection, name):
    info = Gio.DBusNodeInfo.new_for_xml(INTERFACE).interfaces[0]
    connection.register_object("/StatusNotifierWatcher", info, on_call, on_get, None)


Gio.bus_own_name(
    Gio.BusType.SESSION, "org.kde.StatusNotifierWatcher", Gio.BusNameOwnerFlags.NONE, on_bus
)
open(out, "a", encoding="utf-8").close()
GLib.MainLoop().run()
