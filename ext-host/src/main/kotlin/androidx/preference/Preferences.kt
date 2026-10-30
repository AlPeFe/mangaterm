@file:Suppress("unused")

package androidx.preference

import android.content.Context
import android.widget.EditText

// La pantalla de ajustes de la extensión se "dibuja" en memoria: el host la recorre
// para exponer las opciones (y sus valores por defecto) a mangaterm.

open class Preference(private val context: Context) {
    open var key: String? = null
    open var title: CharSequence? = null
    open var summary: CharSequence? = null
    open var defaultValue: Any? = null
    open var isVisible: Boolean = true
    open var isEnabled: Boolean = true
    open var onPreferenceChangeListener: OnPreferenceChangeListener? = null

    open fun getContext(): Context = context

    fun interface OnPreferenceChangeListener {
        fun onPreferenceChange(preference: Preference, newValue: Any?): Boolean
    }
}

abstract class DialogPreference(context: Context) : Preference(context) {
    open var dialogTitle: CharSequence? = null
    open var dialogMessage: CharSequence? = null
}

open class EditTextPreference(context: Context) : DialogPreference(context) {
    open var text: String? = null
    private var bindListener: OnBindEditTextListener? = null

    open fun setOnBindEditTextListener(listener: OnBindEditTextListener?) {
        bindListener = listener
    }

    fun interface OnBindEditTextListener {
        fun onBindEditText(editText: EditText)
    }
}

open class ListPreference(context: Context) : DialogPreference(context) {
    open var entries: Array<CharSequence>? = null
    open var entryValues: Array<CharSequence>? = null
    open var value: String? = null

    open fun findIndexOfValue(value: String?): Int = entryValues?.indexOfFirst { it.toString() == value } ?: -1
}

open class MultiSelectListPreference(context: Context) : DialogPreference(context) {
    open var entries: Array<CharSequence>? = null
    open var entryValues: Array<CharSequence>? = null
    open var values: Set<String> = emptySet()
}

abstract class TwoStatePreference(context: Context) : Preference(context) {
    open var isChecked: Boolean = false
}

open class SwitchPreferenceCompat(context: Context) : TwoStatePreference(context)

open class CheckBoxPreference(context: Context) : TwoStatePreference(context)

open class PreferenceScreen(context: Context) : Preference(context) {
    val preferences = mutableListOf<Preference>()

    open fun addPreference(preference: Preference): Boolean = preferences.add(preference)
}
