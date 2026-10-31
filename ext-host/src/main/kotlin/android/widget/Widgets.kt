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

/** Sin interfaz: el mensaje va al log del host. */
class Toast private constructor(private var text: CharSequence?) {
    fun show() = System.err.println("toast: $text")

    fun setText(s: CharSequence?) {
        text = s
    }

    fun cancel() {}

    companion object {
        const val LENGTH_SHORT = 0
        const val LENGTH_LONG = 1

        @JvmStatic
        fun makeText(context: android.content.Context?, text: CharSequence?, duration: Int) = Toast(text)

        @JvmStatic
        fun makeText(context: android.content.Context?, resId: Int, duration: Int) = Toast(resId.toString())
    }
}
