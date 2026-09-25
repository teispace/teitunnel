package com.teispace.teitunnel

import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionPlaces
import com.intellij.openapi.actionSystem.DataProvider
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.SimpleToolWindowPanel
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.ColoredListCellRenderer
import com.intellij.ui.PopupHandler
import com.intellij.ui.SimpleTextAttributes
import com.intellij.ui.components.JBList
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.content.ContentFactory
import com.teispace.teitunnel.control.Route
import com.teispace.teitunnel.control.Share
import com.teispace.teitunnel.control.shortUrl
import java.awt.event.MouseAdapter
import java.awt.event.MouseEvent
import javax.swing.DefaultListModel
import javax.swing.JList
import javax.swing.ListSelectionModel

/** A row of the tool window: a section title, a share, a route or a note. */
sealed interface Row {
    data class Header(val title: String) : Row
    data class ShareRow(val share: Share) : Row
    data class RouteRow(val route: Route) : Row
    data class Note(val text: String) : Row
}

/** The rows for what the service knows, shares first. */
fun rows(state: TeitunnelService.State, shares: List<Share>, routes: List<Route>, routesProblem: String?): List<Row> {
    if (state != TeitunnelService.State.CONNECTED) return emptyList()
    val out = mutableListOf<Row>(Row.Header("Shares"))
    if (shares.isEmpty()) out += Row.Note("No shares. Use Share Port… to add one.")
    shares.forEach { out += Row.ShareRow(it) }
    out += Row.Header("Routes")
    when {
        routesProblem != null -> out += Row.Note(routesProblem)
        routes.isEmpty() -> out += Row.Note("No routes on this computer.")
        else -> routes.forEach { out += Row.RouteRow(it) }
    }
    return out
}

class TeitunnelToolWindowFactory : ToolWindowFactory, DumbAware {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = TeitunnelPanel()
        toolWindow.contentManager.addContent(ContentFactory.getInstance().createContent(panel, null, false))
        TeitunnelService.get().addListener(toolWindow.disposable) { panel.render() }
        panel.render()
    }
}

private class TeitunnelPanel : SimpleToolWindowPanel(true, true), DataProvider {
    private val model = DefaultListModel<Row>()
    private val list = JBList(model)

    init {
        list.selectionMode = ListSelectionModel.SINGLE_SELECTION
        list.cellRenderer = Renderer()
        val actions = ActionManager.getInstance()
        val toolbar = actions.createActionToolbar(
            ActionPlaces.TOOLWINDOW_TOOLBAR_BAR,
            actions.getAction("Teitunnel.ToolWindowToolbar") as DefaultActionGroup,
            true,
        )
        toolbar.targetComponent = this
        setToolbar(toolbar.component)
        setContent(JBScrollPane(list))
        PopupHandler.installPopupMenu(list, "Teitunnel.RowPopup", ActionPlaces.TOOLWINDOW_POPUP)
        list.addMouseListener(object : MouseAdapter() {
            override fun mouseClicked(e: MouseEvent) {
                if (e.clickCount == 2) (list.selectedValue as? Row.ShareRow)?.share?.url?.let(com.intellij.ide.BrowserUtil::browse)
            }
        })
        list.emptyText.apply {
            clear()
            appendLine("Teitunnel isn't running")
            appendLine("Open Teitunnel", SimpleTextAttributes.LINK_PLAIN_ATTRIBUTES) { openTeitunnel() }
        }
    }

    fun render() {
        val service = TeitunnelService.get()
        val selected = list.selectedValue
        model.clear()
        rows(service.state, service.shares, service.routes, service.routesProblem).forEach(model::addElement)
        list.emptyText.clear()
        when (service.state) {
            TeitunnelService.State.CONNECTING -> list.emptyText.appendLine("Connecting to Teitunnel…")
            else -> {
                list.emptyText.appendLine(service.lastError?.message ?: "Teitunnel isn't running.")
                list.emptyText.appendLine("Open Teitunnel", SimpleTextAttributes.LINK_PLAIN_ATTRIBUTES) { openTeitunnel() }
            }
        }
        if (selected != null) {
            (0 until model.size()).firstOrNull { model[it] == selected }?.let(list::setSelectedIndex)
        }
    }

    override fun getData(dataId: String): Any? = when {
        SELECTED_SHARE.`is`(dataId) -> (list.selectedValue as? Row.ShareRow)?.share
        SELECTED_ROUTE.`is`(dataId) -> (list.selectedValue as? Row.RouteRow)?.route
        else -> null
    }
}

private class Renderer : ColoredListCellRenderer<Row>() {
    override fun customizeCellRenderer(list: JList<out Row>, value: Row?, index: Int, selected: Boolean, hasFocus: Boolean) {
        when (value) {
            is Row.Header -> append(value.title, SimpleTextAttributes.GRAYED_BOLD_ATTRIBUTES)
            is Row.Note -> append(value.text, SimpleTextAttributes.GRAYED_ATTRIBUTES)
            is Row.ShareRow -> {
                val share = value.share
                icon = when {
                    share.live -> Icons.live
                    share.status == "failed" -> Icons.problem
                    else -> Icons.waiting
                }
                append(share.label)
                append("  ${shortUrl(share.origin)} · ${share.status}", SimpleTextAttributes.GRAYED_ATTRIBUTES)
                toolTipText = listOfNotNull(share.url, share.error).joinToString("\n")
            }
            is Row.RouteRow -> {
                val route = value.route
                icon = if (route.status == "live") Icons.live else Icons.problem
                append(route.hostname + (route.path ?: ""))
                append("  ${shortUrl(route.origin)} · ${route.statusText}", SimpleTextAttributes.GRAYED_ATTRIBUTES)
            }
            null -> {}
        }
    }
}
