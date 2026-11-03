/**
 * A narrow chat window (the 700 px layout): number and citation cards stop
 * opening on hover there, so passing over the text never covers it.
 */
export const narrowWindow = () => typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(max-width: 720px)").matches;
