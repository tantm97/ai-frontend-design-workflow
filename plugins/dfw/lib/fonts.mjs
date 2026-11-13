// Font support check against the Google Fonts CSS API (free, no key): which subsets a
// family ships (e.g. "vietnamese"). Only the family name is sent.
export async function fontSubsets(family) {
  const url = `https://fonts.googleapis.com/css2?family=${encodeURIComponent(family).replace(/%20/g, '+')}&display=swap`;
  const res = await fetch(url, { headers: { 'user-agent': 'Mozilla/5.0 (Macintosh) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130 Safari/537.36' } });
  if (!res.ok) return { family, ok: false, status: res.status, subsets: [] };
  const css = await res.text();
  const subsets = [...new Set([...css.matchAll(/\/\*\s*([\w-]+)\s*\*\//g)].map((m) => m[1]))];
  return { family, ok: true, subsets };
}
