import { existsSync, readFileSync, readdirSync, statSync } from "node:fs"
import { dirname, join, relative, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { defineConfig, type DefaultTheme } from "vitepress"

const REPOSITORY = "https://github.com/StevenJPx2/atelier"
const BRANCH = "main"
const SITE = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const ROOT = resolve(SITE, "..")
const SKIPPED = new Set([".agents", ".git", ".github", ".vitepress", "node_modules", "target", "dist"])

/** Every Markdown file in the repository, relative to its root. */
function pages(folder = ROOT): string[] {
  return readdirSync(folder, { withFileTypes: true })
    .filter((entry) => !SKIPPED.has(entry.name) && !entry.name.startsWith("mutants.out"))
    .flatMap((entry) => {
      const path = join(folder, entry.name)
      if (entry.isDirectory()) return pages(path)
      return entry.name.endsWith(".md") ? [relative(ROOT, path)] : []
    })
    .toSorted()
}

/** A folder's `README.md` is its index page, so `crates/core/src/machine/` links work. */
const rewrite = (page: string) => page.replace(/(^|\/)README\.md$/, "$1index.md")

/** Pages with no heading of their own, such as the prompt a builder is sent word for word. */
const TITLES: Record<string, string> = {
  "crates/core/prompts/builder.md": "The builder's contract",
}

function title(page: string): string {
  const heading = readFileSync(join(ROOT, page), "utf8").match(/^# (.+)$/m)?.[1].replaceAll("`", "")
  return TITLES[page] ?? heading ?? page
}

function link(page: string): string {
  return `/${rewrite(page).replace(/(^|\/)index\.md$/, "$1").replace(/\.md$/, "")}`
}

/** A crate's or plugin's own README reads as its name, such as `core`; other pages by their title. */
function crateName(page: string): string {
  const parts = page.split("/")
  return parts.length === 3 ? parts[1] : title(page)
}

function sidebar(all: string[]): DefaultTheme.SidebarItem[] {
  const under = (prefix: string) => all.filter((page) => page.startsWith(prefix))
  const item = (page: string, text = title(page)) => ({ text, link: link(page) })
  /** A folder's pages under its title; a README is the title's own page, not an item. */
  const section = (text: string, folder: string, label = (page: string) => title(page)) => {
    const index = `${folder}README.md`
    // In the order the README links them; pages it does not link come last.
    const readme = all.includes(index) ? readFileSync(join(ROOT, index), "utf8") : ""
    const at = (page: string) => {
      const found = readme.indexOf(`](${page.slice(folder.length)}`)
      return found === -1 ? Number.MAX_SAFE_INTEGER : found
    }
    const pages = under(folder)
      .filter((page) => page !== index)
      .toSorted((a, b) => at(a) - at(b) || a.localeCompare(b))
    const labels = pages.map(label)
    // Two pages with one title, such as each plugin's fixtures, say whose they are.
    const items = pages.map((page, at) => {
      const repeated = labels.filter((other) => other === labels[at]).length > 1
      return item(page, repeated ? `${labels[at]} · ${page.split("/")[1]}` : labels[at])
    })
    return all.includes(index) ? { text, link: link(index), items } : { text, items }
  }
  return [
    { text: "atelier", link: "/" },
    section("Slices", "docs/plans/"),
    section("Crates", "crates/", crateName),
    section("Plugins", "plugins/", crateName),
    section("Audits", "docs/audits/"),
    { text: "Repository", items: [...under("scripts/"), ...under("site/")].map((page) => item(page)) },
  ]
}

/**
 * A relative link to a file the site does not render, such as a `.rs` file
 * or a folder without a README, goes to that path on GitHub instead.
 */
function sourceLink(href: string, from: string): string {
  if (/^([a-z]+:|#|\/)/i.test(href)) return href
  // A folder's README is its index page.
  if (/(^|\/)README\.md(#|$)/.test(href)) return href.replace(/README\.md/, "")
  const [path, hash] = href.split("#")
  const target = resolve(dirname(from), decodeURI(path))
  const rendered = target.endsWith(".md") || existsSync(join(target, "README.md"))
  if (rendered || !target.startsWith(ROOT) || !existsSync(target)) return href
  const kind = statSync(target).isDirectory() ? "tree" : "blob"
  const anchor = hash === undefined ? "" : `#${hash}`
  return `${REPOSITORY}/${kind}/${BRANCH}/${relative(ROOT, target)}${anchor}`
}

const all = pages()

export default defineConfig({
  title: "atelier",
  description: "A personal software factory driven by labelled issues",
  srcDir: ROOT,
  srcExclude: ["**/node_modules/**", "target/**", "mutants.out*/**"],
  rewrites: Object.fromEntries(all.map((page) => [page, rewrite(page)])),
  cleanUrls: true,
  // Pages live outside this folder, so their imports resolve from its node_modules.
  vite: {
    resolve: { alias: [{ find: /^vue(?=$|\/)/, replacement: join(SITE, "node_modules/vue") }] },
    // Mermaid's own chunks reach 674 kB; they load only on a page with a diagram.
    build: { chunkSizeWarningLimit: 700 },
  },
  lastUpdated: true,
  themeConfig: {
    nav: [
      { text: "Plan", link: `${REPOSITORY}/issues/1` },
      { text: "Slices", link: "/docs/plans/" },
      { text: "Crates", link: "/crates/" },
    ],
    sidebar: sidebar(all),
    outline: [2, 3],
    search: { provider: "local" },
    socialLinks: [{ icon: "github", link: REPOSITORY }],
    editLink: { pattern: `${REPOSITORY}/edit/${BRANCH}/:path` },
  },
  markdown: {
    config(md) {
      const fence = md.renderer.rules.fence
      md.renderer.rules.fence = (tokens, at, options, env, self) => {
        const token = tokens[at]
        if (token.info.trim() !== "mermaid") return fence?.(tokens, at, options, env, self) ?? ""
        // v-pre keeps Vue from reading {{ }} in a diagram as a template.
        return `<pre class="mermaid" v-pre>${md.utils.escapeHtml(token.content)}</pre>`
      }
      md.core.ruler.push("source-links", (state) => {
        const from = state.env.path as string | undefined
        if (!from) return
        for (const token of state.tokens.flatMap((block) => block.children ?? [])) {
          const href = token.type === "link_open" ? token.attrGet("href") : null
          if (href) token.attrSet("href", sourceLink(href, from))
        }
      })
    },
  },
})
