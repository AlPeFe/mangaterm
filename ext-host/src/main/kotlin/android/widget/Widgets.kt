@file:Suppress("unused", "UNUSED_PARAMETER")

package android.widget

import android.text.TextWatcher
import android.view.View

open class TextView : View() {
    open fun setHint(hint: CharSequence?) {}
    open fun setError(error: CharSequence?) {}
    open fun getError(): CharSequence? = null
    open fun setHorizontallyScrolling(whether: Boolean) {}
    open fun addTextChangedListener(watcher: TextWatcher) {}
}

open class EditText : TextView() {
    open fun selectAll() {}
}

open class Button : TextView()
