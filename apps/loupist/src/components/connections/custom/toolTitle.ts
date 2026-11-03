/** Plain-language tool name shared by Chat approval notices and the connection review. */
export function humanizeToolName(name: string) {
  const words = name.replace(/[_-]+/g, " ").trim().toLocaleLowerCase();
  return words.charAt(0).toLocaleUpperCase() + words.slice(1);
}
