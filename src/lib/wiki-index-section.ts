// Existing projects can contain headings from before the output language changed.
const koreanSections: Record<string, string> = {
  entities: "개체", concepts: "개념", sources: "자료", queries: "질문", comparisons: "비교", synthesis: "종합",
}

export function insertWikiIndexEntry(index: string, dir: string, entry: string, language: string): string {
  const english = dir.charAt(0).toUpperCase() + dir.slice(1)
  const korean = koreanSections[dir]
  const heading = language === "Korean" && korean ? korean : english
  const content = index || (language === "Korean" ? "# 위키 색인" : "# Wiki Index")
  const lines = content.trimEnd().split(/\r?\n/)
  const section = lines.findIndex(line => line === `## ${english}` || (korean && line === `## ${korean}`))
  if (section < 0) lines.push("", `## ${heading}`, entry)
  else lines.splice(section + 1, 0, entry)
  return lines.join("\n") + "\n"
}
