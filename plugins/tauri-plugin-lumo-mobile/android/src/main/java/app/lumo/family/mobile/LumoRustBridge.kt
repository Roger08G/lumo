package app.lumo.family.mobile

internal object LumoRustBridge {
    init {
        System.loadLibrary("lumo_mobile_lib")
    }

    @JvmStatic external fun processBackgroundTick(payload: String): String
}
