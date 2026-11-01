package app.winklet

import android.annotation.SuppressLint
import android.content.Context
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.ViewConfiguration
import android.widget.FrameLayout
import kotlin.math.hypot

/**
 * Hosts the Compose island in the overlay window. Drags are handled here with raw screen
 * coordinates (the window moves under the finger, so view-local deltas would jitter);
 * taps still reach Compose. A touch outside the window, or Back, closes the panel.
 */
@SuppressLint("ViewConstructor", "ClickableViewAccessibility")
class OverlayRoot(
    context: Context,
    private val canDrag: () -> Boolean,
    private val onDrag: (dx: Int, dy: Int) -> Unit,
    private val onDragEnd: () -> Unit,
    private val onOutside: () -> Unit,
    /** Back while the panel is open closes it; returns whether it was handled. */
    private val onBack: () -> Boolean,
) : FrameLayout(context) {
    private val slop = ViewConfiguration.get(context).scaledTouchSlop
    private var downX = 0f
    private var downY = 0f
    private var lastX = 0f
    private var lastY = 0f
    private var dragging = false

    override fun dispatchTouchEvent(ev: MotionEvent): Boolean {
        if (ev.actionMasked == MotionEvent.ACTION_OUTSIDE) {
            onOutside()
            return true
        }
        return super.dispatchTouchEvent(ev)
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (event.keyCode == KeyEvent.KEYCODE_BACK) {
            if (event.action == KeyEvent.ACTION_UP && onBack()) return true
            if (event.action == KeyEvent.ACTION_DOWN) return true
        }
        return super.dispatchKeyEvent(event)
    }

    override fun onInterceptTouchEvent(ev: MotionEvent): Boolean {
        when (ev.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                downX = ev.rawX
                downY = ev.rawY
                dragging = false
            }
            MotionEvent.ACTION_MOVE -> if (!dragging && canDrag() &&
                hypot(ev.rawX - downX, ev.rawY - downY) > slop
            ) {
                dragging = true
                lastX = ev.rawX
                lastY = ev.rawY
            }
        }
        return dragging
    }

    override fun onTouchEvent(ev: MotionEvent): Boolean {
        when (ev.actionMasked) {
            MotionEvent.ACTION_MOVE -> if (dragging) {
                onDrag((ev.rawX - lastX).toInt(), (ev.rawY - lastY).toInt())
                // keep the remainder so slow drags don't lose sub-pixel movement
                lastX += (ev.rawX - lastX).toInt()
                lastY += (ev.rawY - lastY).toInt()
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> if (dragging) {
                dragging = false
                onDragEnd()
            }
        }
        return true
    }
}
