package com.teispace.teitunnel

import com.teispace.teitunnel.TeitunnelService.State
import com.teispace.teitunnel.control.Route
import com.teispace.teitunnel.control.Share
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PresentationTest {
    private val live = Share("qs-1", "quick", "https://a.trycloudflare.com", "http://localhost:3000", "live", 1, null)
    private val starting = Share("qs-2", "quick", null, "http://localhost:5173", "starting", 0, null)
    private val route = Route("app.example.com", null, "http://localhost:3000", "live", "Live")

    @Test
    fun `status bar counts live shares`() {
        assertEquals("Teitunnel: off", statusText(State.DISCONNECTED, emptyList()))
        assertEquals("Teitunnel", statusText(State.CONNECTED, emptyList()))
        assertEquals("Teitunnel: 1 live", statusText(State.CONNECTED, listOf(live, starting)))
    }

    @Test
    fun `tool window lists shares then routes`() {
        assertTrue(rows(State.DISCONNECTED, listOf(live), listOf(route), null).isEmpty())
        assertEquals(
            listOf(Row.Header("Shares"), Row.ShareRow(live), Row.ShareRow(starting), Row.Header("Routes"), Row.RouteRow(route)),
            rows(State.CONNECTED, listOf(live, starting), listOf(route), null),
        )
        assertEquals(
            listOf(Row.Header("Shares"), Row.Note("No shares. Use Share Port… to add one."), Row.Header("Routes"), Row.Note("No Cloudflare account is connected.")),
            rows(State.CONNECTED, emptyList(), emptyList(), "No Cloudflare account is connected."),
        )
        assertEquals("localhost:5173", starting.label)
        assertEquals("https://app.example.com", route.url)
    }
}
