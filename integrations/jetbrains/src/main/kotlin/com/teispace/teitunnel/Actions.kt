package com.teispace.teitunnel

import com.intellij.icons.AllIcons
import com.intellij.ide.BrowserUtil
import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DataKey
import com.intellij.openapi.ide.CopyPasteManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.util.text.StringUtil
import com.intellij.openapi.wm.WindowManager
import com.teispace.teitunnel.control.ControlException
import com.teispace.teitunnel.control.Protocol
import com.teispace.teitunnel.control.Route
import com.teispace.teitunnel.control.Share
import com.teispace.teitunnel.control.parseOrigin
import com.teispace.teitunnel.control.shortUrl
import java.awt.datatransfer.StringSelection

/** The share or route selected in the tool window. */
val SELECTED_SHARE: DataKey<Share> = DataKey.create("teitunnel.share")
val SELECTED_ROUTE: DataKey<Route> = DataKey.create("teitunnel.route")

private fun notify(project: Project?, text: String, type: NotificationType = NotificationType.INFORMATION, vararg actions: NotificationAction) {
    val notification = NotificationGroupManager.getInstance().getNotificationGroup("Teitunnel").createNotification(text, type)
    actions.forEach(notification::addAction)
    notification.notify(project)
}

/** Brings the Teitunnel app to the front (starting it), then connects. */
fun openTeitunnel() {
    BrowserUtil.browse(Protocol.OPEN_APP_URL)
    TeitunnelService.get().reconnectSoon()
}

/** Explains a failure: quiet for a declined change, with "Open Teitunnel" when the app isn't there. */
fun explain(project: Project?, error: ControlException) {
    when {
        error.declined -> WindowManager.getInstance().getStatusBar(project ?: return)?.info = "Teitunnel: not allowed"
        error.appUnavailable -> notify(
            project,
            error.message ?: "",
            NotificationType.WARNING,
            NotificationAction.createSimpleExpiring("Open Teitunnel") { openTeitunnel() },
        )
        else -> notify(project, error.message ?: "", NotificationType.ERROR)
    }
}

/** Shares `origin` and copies its address. */
fun share(project: Project?, origin: String) {
    TeitunnelService.get().run(
        task = { it.startShare(origin) },
        done = { share ->
            val url = share.url ?: return@run
            CopyPasteManager.getInstance().setContents(StringSelection(url))
            notify(
                project,
                "${shortUrl(share.origin)} is shared at $url (address copied).",
                NotificationType.INFORMATION,
                NotificationAction.createSimpleExpiring("Open in Browser") { BrowserUtil.browse(url) },
                NotificationAction.createSimpleExpiring("Open Inspector") { openInApp(project, mapOf("view" to "inspector", "share" to share.id)) },
            )
        },
        failed = { explain(project, it) },
    )
}

fun openInApp(project: Project?, view: Map<String, String>) {
    TeitunnelService.get().run(task = { it.open(view) }, failed = { explain(project, it) })
}

abstract class TeitunnelAction : AnAction() {
    override fun getActionUpdateThread() = ActionUpdateThread.BGT
}

class SharePortAction : TeitunnelAction() {
    override fun actionPerformed(e: AnActionEvent) {
        val input = Messages.showInputDialog(
            e.project,
            "A port, host:port or local URL. Anyone with the address can open it.",
            "Share a Port with Teitunnel",
            null,
            "3000",
            null,
        ) ?: return
        val origin = parseOrigin(input)
        if (origin == null) {
            Messages.showErrorDialog(e.project, "Enter a port like 3000.", "Share a Port with Teitunnel")
            return
        }
        share(e.project, origin)
    }
}

class StopShareAction : TeitunnelAction() {
    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = e.getData(SELECTED_SHARE) != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        val target = e.getData(SELECTED_SHARE) ?: return
        TeitunnelService.get().run(
            task = { it.stopShare(target.id) },
            done = { WindowManager.getInstance().getStatusBar(e.project ?: return@run)?.info = "Stopped sharing ${shortUrl(target.origin)}" },
            failed = { explain(e.project, it) },
        )
    }
}

class CopyAddressAction : TeitunnelAction() {
    private fun url(e: AnActionEvent) = e.getData(SELECTED_SHARE)?.url ?: e.getData(SELECTED_ROUTE)?.url

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = url(e) != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        val url = url(e) ?: return
        CopyPasteManager.getInstance().setContents(StringSelection(url))
        WindowManager.getInstance().getStatusBar(e.project ?: return)?.info = "Copied ${shortUrl(url)}"
    }
}

class OpenInBrowserAction : TeitunnelAction() {
    private fun url(e: AnActionEvent) = e.getData(SELECTED_SHARE)?.url ?: e.getData(SELECTED_ROUTE)?.url

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = url(e) != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        url(e)?.let(BrowserUtil::browse)
    }
}

class OpenInspectorAction : TeitunnelAction() {
    override fun update(e: AnActionEvent) {
        e.presentation.isEnabled = e.getData(SELECTED_SHARE) != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        val target = e.getData(SELECTED_SHARE) ?: return
        openInApp(e.project, mapOf("view" to "inspector", "share" to target.id))
    }
}

class ShowInTeitunnelAction : TeitunnelAction() {
    override fun actionPerformed(e: AnActionEvent) {
        val share = e.getData(SELECTED_SHARE)
        val route = e.getData(SELECTED_ROUTE)
        openInApp(
            e.project,
            when {
                share != null -> mapOf("view" to "share", "id" to share.id)
                route != null -> mapOf("view" to "route", "hostname" to route.hostname)
                else -> mapOf("view" to "overview")
            },
        )
    }
}

class RunDoctorAction : TeitunnelAction() {
    override fun actionPerformed(e: AnActionEvent) {
        TeitunnelService.get().run(
            task = { it.doctor() },
            done = { issues ->
                if (issues.isEmpty()) {
                    notify(e.project, "Teitunnel's Doctor found no problems.")
                } else {
                    val lines = issues.joinToString("<br>") { "<b>${StringUtil.escapeXmlEntities(it.title)}</b> (${StringUtil.escapeXmlEntities(it.subject)})" }
                    notify(
                        e.project,
                        "Teitunnel's Doctor found ${issues.size} ${if (issues.size == 1) "problem" else "problems"}:<br>$lines",
                        if (issues.any { it.severity == "error" }) NotificationType.WARNING else NotificationType.INFORMATION,
                        NotificationAction.createSimpleExpiring("Fix in Teitunnel") { openInApp(e.project, mapOf("view" to "doctor")) },
                    )
                }
            },
            failed = { explain(e.project, it) },
        )
    }
}

class RefreshAction : TeitunnelAction() {
    override fun actionPerformed(e: AnActionEvent) = TeitunnelService.get().reconnectSoon()
}

class OpenTeitunnelAction : TeitunnelAction() {
    override fun actionPerformed(e: AnActionEvent) = openTeitunnel()
}

/** Icons for share and route rows. */
object Icons {
    val live = AllIcons.General.Web
    val waiting = AllIcons.Process.Step_1
    val problem = AllIcons.General.Warning
}
