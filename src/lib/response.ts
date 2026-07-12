const allowedTags = new Set(["A", "BR", "DIV", "EM", "LI", "OL", "P", "SPAN", "STRONG", "UL"]);

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  })[character] ?? character);
}

export function sanitizeSearchSuggestionsHtml(html: string): string {
  if (typeof DOMParser === "undefined") return `<p>${escapeHtml(html)}</p>`;
  const document = new DOMParser().parseFromString(html, "text/html");

  [...document.body.querySelectorAll("*")].forEach((element) => {
    if (!allowedTags.has(element.tagName)) {
      element.replaceWith(document.createTextNode(element.textContent ?? ""));
      return;
    }

    const rawHref = element.tagName === "A" ? element.getAttribute("href") ?? "" : "";
    [...element.attributes].forEach((attribute) => element.removeAttribute(attribute.name));
    if (element.tagName === "A") {
      if (/^https?:\/\//i.test(rawHref)) {
        element.setAttribute("href", rawHref);
        element.setAttribute("rel", "noreferrer");
      }
    }
  });

  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src 'none'; font-src 'none'; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'"><style>body{margin:0;padding:10px;font:12px/1.5 system-ui,sans-serif;color:#343a3f;background:#fff}strong{font-weight:650}ul,ol{margin:6px 0;padding-left:20px}li{margin:3px 0}a{color:#2563a8;text-decoration:none}a:hover{text-decoration:underline}</style></head><body>${document.body.innerHTML}</body></html>`;
}

export function formatBytes(bytes: number | undefined): string {
  if (bytes === undefined) return "-";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
