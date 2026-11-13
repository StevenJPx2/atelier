# Site

The documentation site: every Markdown file in the repository, rendered with
[VitePress](https://vitepress.dev), with Mermaid diagrams drawn and local
search. Readers find it in the Workhorse hub, at
[/atelier/](https://workhorse-docs.stevenjpx2.workers.dev/atelier/), which
pulls these pages from `main` when it builds. Here it runs locally and checks
every pull request. The Worker `atelier-docs`, at
[atelier-docs.stevenjpx2.workers.dev](https://atelier-docs.stevenjpx2.workers.dev),
redirects readers to the corresponding page in the hub. Paths under
`/plans/` and `/audits/` map to `/atelier/docs/plans/` and
`/atelier/docs/audits/`; repository-relative paths keep their path under
`/atelier/`.

| Path                           | Holds                                                                                                       |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| `.vitepress/config.mts`        | Finds the pages, builds the sidebar, makes each folder's `README.md` its index, points code links at GitHub |
| `.vitepress/theme/index.ts`    | Draws `mermaid` blocks in the site's font and colours, again when the page or the colour scheme changes     |
| `.vitepress/theme/mermaid.css` | How a diagram sits in its column, and opened full screen                                                    |
| `cloudflare.config.ts`         | The Worker and its account; `wrangler.config.ts` names the folder it serves                                 |
| `redirect/_redirects`          | Maps legacy plan and audit paths to `/atelier/docs/`, and repository-relative paths to `/atelier/`          |

- **Run:** `bun install`, then `bun run dev`.
- **Build:** `bun run build` writes `.vitepress/dist/`, and fails on a dead
  link.
- **Deploy:** `.github/workflows/docs.yml` builds every pull request and,
  on each push to `main`, deploys the redirect with `CLOUDFLARE_API_TOKEN`.
- A new Markdown file anywhere in the repository appears in the sidebar on
  the next build; nothing here lists pages by hand.
- **Diagrams** use Mermaid's `base` theme with the site's font and colours,
  light or dark. Each fits its column, but never below 75% of its natural
  size, so its text stays readable; a wider one scrolls sideways. Clicking a
  diagram opens it full screen; a click or Escape closes it.
