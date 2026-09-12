package app.lumo.family.mobile

import android.location.LocationListener
import android.os.Bundle
import java.lang.reflect.Modifier
import org.junit.Assert.assertFalse
import org.junit.Test

class LumoLocationCompatibilityTest {
    @Test
    fun legacyProviderCallbacksNeverDependOnAndroid30PlatformDefaults() {
        // The compile SDK exposes defaults that do not exist on Android 7-10.
        // Resolve the service's actual callback implementation without creating a Service.
        val service = LumoLocationService::class.java
        val callbacks = listOf(
            service.getMethod("onProviderEnabled", String::class.java),
            service.getMethod("onProviderDisabled", String::class.java),
            service.getMethod(
                "onStatusChanged",
                String::class.java,
                Int::class.javaPrimitiveType,
                Bundle::class.java,
            ),
        )
        callbacks.forEach { callback ->
            assertFalse(
                "${callback.name} must have an application or AndroidX implementation before API 30",
                callback.declaringClass == LocationListener::class.java,
            )
            assertFalse(Modifier.isAbstract(callback.modifiers))
        }
    }
}
