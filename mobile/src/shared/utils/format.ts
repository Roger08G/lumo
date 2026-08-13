export function formatClock(isoDate: string) {
    const date = new Date(isoDate);
    if (Number.isNaN(date.getTime())) return "--:--";
    return new Intl.DateTimeFormat("es-ES", {
        hour: "2-digit",
        minute: "2-digit",
    }).format(date);
}

export function formatRelative(isoDate: string) {
    const date = new Date(isoDate);
    if (Number.isNaN(date.getTime())) return "sin datos";
    const minutes = Math.max(0, Math.round((Date.now() - date.getTime()) / 60_000));
    if (minutes < 1) return "ahora";
    if (minutes === 1) return "hace 1 min";
    if (minutes < 60) return `hace ${minutes} min`;
    const hours = Math.round(minutes / 60);
    if (hours === 1) return "hace 1 h";
    if (hours < 24) return `hace ${hours} h`;
    return new Intl.DateTimeFormat("es-ES", { day: "numeric", month: "short" }).format(date);
}

export function greeting() {
    const hour = new Date().getHours();
    if (hour < 12) return "Buenos días";
    if (hour < 20) return "Buenas tardes";
    return "Buenas noches";
}
