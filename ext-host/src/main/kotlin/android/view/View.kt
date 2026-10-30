@file:Suppress("unused", "UNUSED_PARAMETER")

// Vistas que aparecen en los diálogos de ajustes. El host no tiene UI de Android: existen solo para enlazar.

package android.view

open class View {
    open fun findViewById(id: Int): View? = null
    open fun getRootView(): View = this
    open fun post(action: Runnable): Boolean = false
    open fun setEnabled(enabled: Boolean) {}
}
