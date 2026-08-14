const VIEWPORT_WIDTH = "--lumo-viewport-width";
const VIEWPORT_HEIGHT = "--lumo-viewport-height";
const VIEWPORT_OFFSET_LEFT = "--lumo-viewport-offset-left";
const VIEWPORT_OFFSET_TOP = "--lumo-viewport-offset-top";

export function installViewportVariables() {
    const root = document.documentElement;
    let animationFrame = 0;

    const update = () => {
        window.cancelAnimationFrame(animationFrame);
        animationFrame = window.requestAnimationFrame(() => {
            const viewport = window.visualViewport;
            root.style.setProperty(
                VIEWPORT_WIDTH,
                `${Math.round(viewport?.width ?? window.innerWidth)}px`,
            );
            root.style.setProperty(
                VIEWPORT_HEIGHT,
                `${Math.round(viewport?.height ?? window.innerHeight)}px`,
            );
            root.style.setProperty(
                VIEWPORT_OFFSET_LEFT,
                `${Math.round(viewport?.offsetLeft ?? 0)}px`,
            );
            root.style.setProperty(
                VIEWPORT_OFFSET_TOP,
                `${Math.round(viewport?.offsetTop ?? 0)}px`,
            );
        });
    };

    update();
    window.addEventListener("resize", update, { passive: true });
    window.addEventListener("orientationchange", update, { passive: true });
    window.visualViewport?.addEventListener("resize", update, { passive: true });
    window.visualViewport?.addEventListener("scroll", update, { passive: true });
}
