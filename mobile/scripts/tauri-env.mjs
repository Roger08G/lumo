import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";

const env = { ...process.env };

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
    env.NDK_HOME = "C:\\.android\\sdk\\ndk\\29.0.13846066";
    env.JAVA_HOME = "C:\\Program Files\\Eclipse Adoptium\\jdk-17.0.20.8-hotspot";
}

const cli = resolve("node_modules/.bin/tauri.exe");
const command = existsSync(cli) ? cli : "tauri";
const result = spawnSync(command, process.argv.slice(2), {
    cwd: process.cwd(),
    env,
    stdio: "inherit",
    windowsHide: false,
});

if (result.error) {
    console.error(result.error.message);
    process.exit(1);
}

process.exit(result.status ?? 1);
