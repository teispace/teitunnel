package com.teispace.teitunnel

import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.StatusBar
import com.intellij.openapi.wm.StatusBarWidget
import com.intellij.openapi.wm.StatusBarWidgetFactory
import com.intellij.openapi.wm.ToolWindowManager
import com.intellij.util.Consumer
import com.teispace.teitunnel.control.Share
import java.awt.Component
import java.awt.event.MouseEvent

/** The status bar text: live share count, or that the app isn't there. */
fun statusText(state: TeitunnelService.State, shares: List<Share>): String = when {
    state != TeitunnelService.State.CONNECTED -> "Teitunnel: off"
    shares.isEmpty() -> "Teitunnel"
    else -> "Teitunnel: ${shares.count { it.live }} live"
}

class TeitunnelStatusBarFactory : StatusBarWidgetFactory {
    override fun getId() = ID

    override fun getDisplayName() = "Teitunnel"

    override fun createWidget(project: Project): StatusBarWidget = Widget(project)

    companion object {
        const val ID = "Teitunnel"
    }

    private class Widget(private val project: Project) : StatusBarWidget, StatusBarWidget.TextPresentation {
        private var bar: StatusBar? = null

        override fun ID() = ID

        override fun install(statusBar: StatusBar) {
            bar = statusBar
            TeitunnelService.get().addListener(this) { statusBar.updateWidget(ID) }
        }

        override fun getPresentation() = this

        override fun getText(): String = TeitunnelService.get().let { statusText(it.state, it.shares) }

        override fun getAlignment() = Component.CENTER_ALIGNMENT

        override fun getTooltipText(): String {
            val service = TeitunnelService.get()
            if (service.state != TeitunnelService.State.CONNECTED) return "Teitunnel isn't running. Click to open it."
            if (service.shares.isEmpty()) return "No shares"
            return service.shares.joinToString("\n") { "${it.label} → ${it.origin} (${it.status})" }
        }

        override fun getClickConsumer(): Consumer<MouseEvent> = Consumer {
            if (TeitunnelService.get().state == TeitunnelService.State.CONNECTED) {
                ToolWindowManager.getInstance(project).getToolWindow("Teitunnel")?.show()
            } else {
                openTeitunnel()
            }
        }

        override fun dispose() {
            // Listeners registered with this widget are disposed with it.
            bar = null
        }
    }
}
