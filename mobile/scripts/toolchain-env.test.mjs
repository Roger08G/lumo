import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import { windowsToolchainEnvironment } from "./toolchain-env.mjs";

describe("portable Windows build environment", () => {
    test("preserves configured toolchains and leaves the source untouched", () => {
        const original = {
            ANDROID_HOME: "custom-sdk",
            ANDROID_SDK_ROOT: "custom-sdk",
            NDK_HOME: "custom-ndk",
            JAVA_HOME: "custom-java",
            RUSTUP_HOME: "custom-rust",
        };
        expect(windowsToolchainEnvironment(original, () => true)).toEqual(original);
        expect(original.NDK_HOME).toBe("custom-ndk");
    });

    test("does not inject nonexistent personal SDK or Java paths", () => {
        expect(windowsToolchainEnvironment({}, () => false)).toEqual({});
    });

    test("discovers an installed NDK using numeric version order", () => {
        const env = windowsToolchainEnvironment(
            { ANDROID_HOME: "sdk" },
            () => true,
            () => ["9.0.123", "29.0.13846066", "README"],
        );
        expect(env.NDK_HOME).toBe(join("sdk", "ndk", "29.0.13846066"));
    });
});
