import { expect, test } from "bun:test";
import { parseCoordinates } from "./coordinates.ts";

test("coordinates accept decimal points and unambiguous Spanish decimal commas", () => {
    expect(parseCoordinates("40.4168, -3.7038")).toEqual({ latitude: 40.4168, longitude: -3.7038 });
    expect(parseCoordinates("40,4168; -3,7038")).toEqual({ latitude: 40.4168, longitude: -3.7038 });
    expect(parseCoordinates("40,4168 -3,7038")).toEqual({ latitude: 40.4168, longitude: -3.7038 });
});

test("coordinates reject malformed text, extra numbers and out-of-range positions", () => {
    for (const value of [
        "text 40.4, -3.7",
        "40.4, -3.7 trailing",
        "40, 3, 7",
        "91, 0",
        "0, -181",
        "NaN, 3",
        "",
        "40e2, 3",
    ]) {
        expect(parseCoordinates(value)).toBeNull();
    }
});
