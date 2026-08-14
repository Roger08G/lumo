package app.lumo.family.mobile

import android.webkit.WebView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import kotlin.math.ceil

private const val ZERO_DENSITY = 1f

internal object LumoSystemInsets {
    fun install(webView: WebView) {
        ViewCompat.setOnApplyWindowInsetsListener(webView) { _, windowInsets ->
            applyToDocument(webView, windowInsets)
            windowInsets
        }
        webView.post { ViewCompat.requestApplyInsets(webView) }
    }

    fun refresh(webView: WebView) {
        webView.post {
            val windowInsets = ViewCompat.getRootWindowInsets(webView)
            if (windowInsets == null) {
                ViewCompat.requestApplyInsets(webView)
            } else {
                applyToDocument(webView, windowInsets)
            }
        }
    }

    private fun toCssPixels(value: Int, density: Float): Int =
        ceil(value / density.coerceAtLeast(ZERO_DENSITY)).toInt()

    private fun applyToDocument(webView: WebView, windowInsets: WindowInsetsCompat) {
        val insets =
            windowInsets.getInsets(
                WindowInsetsCompat.Type.systemBars() or
                    WindowInsetsCompat.Type.displayCutout(),
            )
        val density = webView.resources.displayMetrics.density.coerceAtLeast(ZERO_DENSITY)
        val top = toCssPixels(insets.top, density)
        val right = toCssPixels(insets.right, density)
        val bottom = toCssPixels(insets.bottom, density)
        val left = toCssPixels(insets.left, density)
        val script =
            """
            (() => {
              const root = document.documentElement;
              if (!root) return;
              root.style.setProperty('--lumo-native-safe-top', '${top}px');
              root.style.setProperty('--lumo-native-safe-right', '${right}px');
              root.style.setProperty('--lumo-native-safe-bottom', '${bottom}px');
              root.style.setProperty('--lumo-native-safe-left', '${left}px');
              window.dispatchEvent(new Event('lumo:safe-area-change'));
            })();
            """.trimIndent()
        webView.post { webView.evaluateJavascript(script, null) }
    }
}
