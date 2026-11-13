const NO_CACHE = "no-cache";
const IMMUTABLE = "public, max-age=31536000, immutable";

/** Cache and content-type headers for a static response path; sw.js, index.html and the manifest always revalidate. */
export function staticHeaders(pathname: string): Record<string, string> {
  if (pathname === "/manifest.webmanifest") {
    return { "Content-Type": "application/manifest+json", "Cache-Control": NO_CACHE };
  }
  if (pathname === "/" || pathname === "/sw.js" || pathname.endsWith(".html")) return { "Cache-Control": NO_CACHE };
  if (pathname.startsWith("/assets/")) return { "Cache-Control": IMMUTABLE };
  return { "Cache-Control": NO_CACHE };
}
