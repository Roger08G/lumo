import { existsSync, readdirSync } from "node:fs";
import { delimiter, join } from "node:path";

// Respect the developer's toolchain. Legacy locations are optional fallbacks,
// never a reason to replace a configured SDK, NDK, Java or Rust installation.
export function windowsToolchainEnvironment(source, exists = existsSync, entries = readdirSync) {
    const env = { ...source };
    const useFallback = (name, candidates) => {
        if (env[name]) return;
        const candidate = candidates.find((value) => value && exists(value));
        if (candidate) env[name] = candidate;
    };

    useFallback("ANDROID_HOME", [
        env.ANDROID_SDK_ROOT,
        env.LOCALAPPDATA && join(env.LOCALAPPDATA, "Android", "Sdk"),
        "C:\\.android\\sdk",
    ]);
    useFallback("ANDROID_SDK_ROOT", [env.ANDROID_HOME]);
    useFallback("JAVA_HOME", [
        env.ProgramFiles && join(env.ProgramFiles, "Android", "Android Studio", "jbr"),
    ]);
    if (!env.NDK_HOME && env.ANDROID_HOME) {
        const ndkRoot = join(env.ANDROID_HOME, "ndk");
        if (exists(ndkRoot)) {
            const versions = entries(ndkRoot)
                .filter((name) => /^\d+\.\d+\.\d+$/.test(name))
                .sort((first, second) => second.localeCompare(first, undefined, { numeric: true }));
            useFallback(
                "NDK_HOME",
                versions.map((version) => join(ndkRoot, version)),
            );
        }
    }
    // Add an explicitly configured Cargo home without inventing RUSTUP_HOME.
    if (env.CARGO_HOME) {
        const cargoBin = join(env.CARGO_HOME, "bin");
        if (exists(cargoBin)) {
            const pathKey = Object.keys(env).find((key) => key.toLowerCase() === "path") ?? "Path";
            env[pathKey] = `${cargoBin}${delimiter}${env[pathKey] ?? ""}`;
        }
    }
    return env;
}
