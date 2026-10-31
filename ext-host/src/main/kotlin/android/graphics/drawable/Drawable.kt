@file:Suppress("unused")

package android.graphics.drawable

abstract class Drawable

class ColorDrawable(@JvmField var color: Int = 0) : Drawable()
