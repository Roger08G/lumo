export interface Coordinates {
    latitude: number;
    longitude: number;
}

const COORDINATE_PAIR = /^([+-]?\d+(?:\.\d+)?)\s*,\s*([+-]?\d+(?:\.\d+)?)$/;
const LOCALIZED_COORDINATE_PAIR = /^([+-]?\d+(?:[.,]\d+)?)(?:\s*;\s*|\s+)([+-]?\d+(?:[.,]\d+)?)$/;

export function parseCoordinates(value: string): Coordinates | null {
    const text = value.trim();
    const matches = text.match(COORDINATE_PAIR) ?? text.match(LOCALIZED_COORDINATE_PAIR);
    if (!matches) return null;

    const latitude = Number(matches[1].replace(",", "."));
    const longitude = Number(matches[2].replace(",", "."));
    if (
        !Number.isFinite(latitude) ||
        !Number.isFinite(longitude) ||
        latitude < -90 ||
        latitude > 90 ||
        longitude < -180 ||
        longitude > 180
    ) {
        return null;
    }

    return { latitude, longitude };
}

export function formatCoordinates({ latitude, longitude }: Coordinates) {
    return `${latitude.toFixed(6)}, ${longitude.toFixed(6)}`;
}
