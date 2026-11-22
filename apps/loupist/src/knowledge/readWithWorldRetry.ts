import { commandErrorCode } from "../lib/errors";

/** Let a short-lived store lock clear before surfacing a failed World read. */
export async function readWithWorldRetry<T>(read: () => Promise<T>, active = () => true): Promise<T> {
  const delays = [80, 240];
  for (const delay of delays) {
    try {
      return await read();
    } catch (cause) {
      if (commandErrorCode(cause) !== "world_busy" || !active()) throw cause;
      await new Promise(resolve => setTimeout(resolve, delay));
      if (!active()) throw cause;
    }
  }
  return read();
}
