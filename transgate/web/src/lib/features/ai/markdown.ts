// The source's `formatSimpleMarkdown` for finished assistant answers: bold,
// italic, `# ` headings and `- `/`1. ` lists. One deliberate change: the answer
// is HTML-escaped first, so model or server text can never become markup.

const ESCAPES: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };

/** Consecutive list items become one `<ul>`, as the source's `wrapListItems`. */
function wrapListItems(html: string): string {
  const out: string[] = [];
  let open = false;
  for (const line of html.split("\n")) {
    const item = line.startsWith("<li>") && line.endsWith("</li>");
    if (item && !open) out.push("<ul>");
    if (!item && open) out.push("</ul>");
    open = item;
    out.push(line);
  }
  if (open) out.push("</ul>");
  return out.join("\n");
}

export function simpleMarkdown(text: string): string {
  if (!text) return "";
  let html = text.replace(/[&<>"']/g, (c) => ESCAPES[c]);
  html = html.replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>");
  html = html.replace(/(?<!\*)\*([^*]+?)\*(?!\*)/g, "<em>$1</em>");
  html = html.replace(/^# (.+)$/gm, "<h3>$1</h3>");
  html = html.replace(/^- (.+)$/gm, "<li>$1</li>");
  html = html.replace(/^\d+\. (.+)$/gm, "<li>$1</li>");
  return wrapListItems(html);
}
