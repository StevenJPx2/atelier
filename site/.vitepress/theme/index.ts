import DefaultTheme from "vitepress/theme"
import { useData, useRoute } from "vitepress"
import { nextTick, onMounted, watch } from "vue"

import "./mermaid.css"

/** `color` as hex, any transparency blended over `under`, since Mermaid's themes take hex alone. */
function hex(color: string, under = "#ffffff"): string {
  const probe = document.createElement("span")
  document.body.append(probe)
  const rgba = (value: string) => {
    probe.style.color = value
    return (getComputedStyle(probe).color.match(/[\d.]+/g) ?? ["0", "0", "0"]).map(Number)
  }
  const [r, g, b, a = 1] = rgba(color)
  const [br, bg, bb] = rgba(under)
  probe.remove()
  const mix = (top: number, bottom: number) => Math.round(top * a + bottom * (1 - a))
  return `#${[mix(r, br), mix(g, bg), mix(b, bb)].map((part) => part.toString(16).padStart(2, "0")).join("")}`
}

/** Mermaid's base theme in the site's own font and colours, light or dark as the page is. */
function themeVariables(dark: boolean) {
  const css = getComputedStyle(document.documentElement)
  const page = hex(css.getPropertyValue("--vp-c-bg").trim())
  const color = (name: string) => hex(css.getPropertyValue(name).trim(), page)
  return {
    darkMode: dark,
    fontFamily: getComputedStyle(document.body).fontFamily,
    fontSize: "15px",
    background: page,
    primaryColor: color("--vp-c-bg-soft"),
    primaryTextColor: color("--vp-c-text-1"),
    primaryBorderColor: color("--vp-c-brand-1"),
    secondaryColor: color("--vp-c-brand-soft"),
    tertiaryColor: color("--vp-c-bg-alt"),
    lineColor: color("--vp-c-text-2"),
    textColor: color("--vp-c-text-1"),
    edgeLabelBackground: page,
    clusterBkg: color("--vp-c-bg-alt"),
    clusterBorder: color("--vp-c-divider"),
    noteBkgColor: color("--vp-c-bg-alt"),
    noteTextColor: color("--vp-c-text-1"),
    noteBorderColor: color("--vp-c-divider"),
  }
}

/** The smallest a diagram is shrunk to fit, so its 15 px text stays readable; a wider one scrolls. */
const SMALLEST = 0.75

/** Fits each drawn diagram to its column, never below `SMALLEST` of its natural size. */
function fit() {
  for (const block of document.querySelectorAll<HTMLElement>("pre.mermaid")) {
    const svg = block.querySelector("svg")
    const natural = Number.parseFloat(svg?.getAttribute("width") ?? "")
    if (!svg || !natural) continue
    const scale = Math.min(1, Math.max(SMALLEST, block.clientWidth / natural))
    svg.style.width = `${natural * scale}px`
  }
}

/**
 * Draws every `pre.mermaid` block on the page, then fits each to its column.
 * Each keeps its source, so a switch between light and dark draws it again
 * in the new colours.
 */
async function draw(dark: boolean) {
  const blocks = [...document.querySelectorAll<HTMLElement>("pre.mermaid")]
  if (blocks.length === 0) return
  for (const block of blocks) {
    block.dataset.source ??= block.textContent ?? ""
    block.textContent = block.dataset.source
    block.removeAttribute("data-processed")
  }
  const { default: mermaid } = await import("mermaid")
  const natural = { useMaxWidth: false }
  mermaid.initialize({
    startOnLoad: false,
    theme: "base",
    themeVariables: themeVariables(dark),
    flowchart: { ...natural, padding: 12 },
    sequence: natural,
    state: natural,
    er: natural,
  })
  await mermaid.run({ nodes: blocks })
  fit()
}

/** Opens a drawn diagram full screen, scaled to fit; a click or Escape closes it. */
function enlarge(event: MouseEvent) {
  const svg = (event.target as Element | null)?.closest("pre.mermaid")?.querySelector("svg")
  if (!svg) return
  const overlay = document.createElement("div")
  overlay.className = "mermaid-overlay"
  overlay.setAttribute("role", "dialog")
  overlay.setAttribute("aria-label", "Diagram, full screen")
  overlay.append(svg.cloneNode(true))
  const close = () => {
    overlay.remove()
    document.removeEventListener("keydown", onKey)
  }
  const onKey = (key: KeyboardEvent) => key.key === "Escape" && close()
  overlay.addEventListener("click", close)
  document.addEventListener("keydown", onKey)
  document.body.append(overlay)
}

export default {
  extends: DefaultTheme,
  setup() {
    const route = useRoute()
    const { isDark } = useData()
    const redraw = () => nextTick(() => draw(isDark.value))
    onMounted(() => {
      redraw()
      document.addEventListener("click", enlarge)
      window.addEventListener("resize", fit)
    })
    watch(() => route.path, redraw)
    watch(isDark, redraw)
  },
}
