import { spawnSync } from "node:child_process";
import {
    appendFileSync,
    cpSync,
    existsSync,
    readFileSync,
    readdirSync,
    writeFileSync,
} from "node:fs";
import { resolve } from "node:path";

const env = { ...process.env };
const rootEnvPath = resolve("../.env");
const rootEnv = existsSync(rootEnvPath) ? readFileSync(rootEnvPath, "utf8") : "";

function configValue(key) {
    const direct = env[key]?.trim();
    if (direct) return direct;
    const match = rootEnv.match(new RegExp(`^${key}\\s*=\\s*(.+?)\\s*$`, "m"));
    return match?.[1].replace(/^(?:"([\s\S]*)"|'([\s\S]*)')$/, "$1$2").trim() ?? null;
}

function publicApiOrigin() {
    const value = configValue("LUMO_API_URL");
    if (!value) return null;
    try {
        const url = new URL(value);
        if (url.protocol === "https:" && url.username === "" && url.password === "") {
            return url.origin;
        }
    } catch {
        // Rust configuration reports the authoritative error without exposing other .env values.
    }
    return null;
}

const apiOrigin = publicApiOrigin();
if (apiOrigin) env.VITE_LUMO_API_ORIGIN = apiOrigin;

if (
    process.argv.includes("build") &&
    configValue("LUMO_RUNTIME_MODE")?.toLowerCase() === "remote" &&
    !apiOrigin
) {
    console.error("Remote builds require a valid HTTPS LUMO_API_URL for QR origin validation.");
    process.exit(1);
}

function syncAndroidProject() {
    const tauriConfigPath = resolve("src-tauri/tauri.conf.json");
    const stringsPath = resolve("src-tauri/gen/android/app/src/main/res/values/strings.xml");
    if (existsSync(tauriConfigPath) && existsSync(stringsPath)) {
        const { productName = "Lumo" } = JSON.parse(readFileSync(tauriConfigPath, "utf8"));
        const escapedProductName = String(productName)
            .replaceAll("&", "&amp;")
            .replaceAll("<", "&lt;")
            .replaceAll(">", "&gt;")
            .replaceAll('"', "&quot;")
            .replaceAll("'", "&apos;");
        const originalStrings = readFileSync(stringsPath, "utf8");
        const strings = originalStrings
            .replace(
                /(<string name="app_name">)[\s\S]*?(<\/string>)/,
                `$1"${escapedProductName}"$2`,
            )
            .replace(
                /(<string name="main_activity_title">)[\s\S]*?(<\/string>)/,
                `$1"${escapedProductName}"$2`,
            );
        if (strings !== originalStrings) writeFileSync(stringsPath, strings, "utf8");
    }

    const iconSource = resolve("src-tauri/icons/android");
    const resourceTarget = resolve("src-tauri/gen/android/app/src/main/res");
    if (existsSync(iconSource) && existsSync(resourceTarget)) {
        for (const entry of readdirSync(iconSource, { withFileTypes: true })) {
            cpSync(resolve(iconSource, entry.name), resolve(resourceTarget, entry.name), {
                recursive: true,
                force: true,
            });
        }
    }

    const manifestPath = resolve("src-tauri/gen/android/app/src/main/AndroidManifest.xml");
    if (!existsSync(manifestPath)) return;

    const originalManifest = readFileSync(manifestPath, "utf8");
    let manifest = originalManifest;
    if (!/android:roundIcon=/.test(manifest)) {
        manifest = manifest.replace(
            /(<application\s+android:icon="@mipmap\/ic_launcher")/,
            '$1\n        android:roundIcon="@mipmap/ic_launcher_round"',
        );
    }
    if (!/android:windowSoftInputMode=/.test(manifest)) {
        manifest = manifest.replace(
            /(<activity\s+android:configChanges="[^"]+")/,
            '$1\n            android:windowSoftInputMode="adjustResize"',
        );
    }
    if (manifest !== originalManifest) writeFileSync(manifestPath, manifest, "utf8");
}

syncAndroidProject();

if (process.platform === "win32") {
    const cargoBin = "C:\\.android\\cargo\\bin";
    const pathKey = Object.keys(env).find((key) => key.toLowerCase() === "path") ?? "Path";
    env[pathKey] = `${cargoBin};${env[pathKey] ?? ""}`;
    env.RUSTUP_HOME = "C:\\.android\\rustup";
    env.CARGO_HOME = "C:\\.android\\cargo";
    env.CARGO_TARGET_DIR = "C:\\.android\\lumo-target";
    env.GRADLE_USER_HOME = "C:\\.android\\gradle";
    env.ANDROID_HOME = "C:\\.android\\sdk";
    env.ANDROID_SDK_ROOT = "C:\\.android\\sdk";
    env.ANDROID_AVD_HOME = "C:\\.android\\avd";
    env.NDK_HOME = "C:\\.android\\sdk\\ndk\\29.0.13846066";
    env.JAVA_HOME = "C:\\Program Files\\Eclipse Adoptium\\jdk-17.0.20.8-hotspot";
    env.CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER =
        "C:\\.android\\sdk\\ndk\\29.0.13846066\\toolchains\\llvm\\prebuilt\\windows-x86_64\\bin\\aarch64-linux-android24-clang.cmd";
    env.CC_AARCH64_LINUX_ANDROID = env.CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER;
    env.AR_AARCH64_LINUX_ANDROID =
        "C:\\.android\\sdk\\ndk\\29.0.13846066\\toolchains\\llvm\\prebuilt\\windows-x86_64\\bin\\llvm-ar.exe";
    const gradleProperties = resolve("src-tauri/gen/android/gradle.properties");
    if (
        existsSync(gradleProperties) &&
        !/^android\.overridePathCheck\s*=\s*true\s*$/m.test(readFileSync(gradleProperties, "utf8"))
    ) {
        appendFileSync(gradleProperties, "\nandroid.overridePathCheck=true\n", "utf8");
    }
}

const cli = resolve("node_modules/.bin/tauri.exe");
const command = existsSync(cli) ? cli : "tauri";
const result = spawnSync(command, process.argv.slice(2), {
    cwd: process.cwd(),
    env,
    stdio: "inherit",
    windowsHide: false,
});

if (process.argv.includes("init")) syncAndroidProject();

if (result.error) {
    console.error(result.error.message);
    process.exit(1);
}

process.exit(result.status ?? 1);
