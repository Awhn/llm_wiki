import { useWikiStore } from "@/stores/wiki-store"
import { detectLanguage } from "./detect-language"
import { getLanguagePromptName } from "./language-metadata"

/**
 * Get the effective output language for LLM content generation.
 *
 * If user has explicitly set an outputLanguage, use it.
 * Otherwise (auto), fall back to detecting the language from the given text.
 */
export function getOutputLanguage(fallbackText: string = ""): string {
  const configured = useWikiStore.getState().outputLanguage
  if (configured && configured !== "auto") {
    return configured
  }
  return detectLanguage(fallbackText || "English")
}

/**
 * Build a strong language directive to inject into system prompts.
 */
export function buildLanguageDirective(fallbackText: string = ""): string {
  const lang = getOutputLanguage(fallbackText)
  const promptLang = getLanguagePromptName(lang)
  if (lang === "Korean") {
    return [
      "## ⚠️ MANDATORY OUTPUT LANGUAGE: Korean",
      "독자가 읽는 제목, 소제목, 본문, 표 머리글, 요약, 그림 설명, 주제 태그를 자연스러운 한국어로 쓴다. 원문이나 예시가 영어여도 같은 원칙을 적용한다.",
      "일반 개념은 통용되는 한국어 용어 하나로 일관되게 쓴다. 예를 들어 ‘cache hit rate를 monitor한다’는 ‘캐시 적중률을 확인한다’로, ‘Key Findings’는 ‘주요 결과’로 쓴다. 익숙한 용어에 영문을 괄호로 반복하지 않는다.",
      "공식 명칭, 약어, 코드 식별자, 파일명, URL, 인용문과 서지 제목은 정확하게 보존한다. 예를 들어 PostgreSQL과 CREATE TABLE은 그대로 쓴다. 정착된 번역이 없거나 원어가 개념 구별에 꼭 필요하면 첫 등장에만 원어를 병기한다.",
      "YAML 키와 type 값, FILE/REVIEW 구분자와 제어 레이블, 경로, 기존 위키 링크 대상은 원형을 유지한다. 형식 예시의 영어 안내문이나 내부 분석 레이블을 문서 본문에 옮기지 않는다.",
    ].join("\n")
  }
  return [
    `## ⚠️ MANDATORY OUTPUT LANGUAGE: ${promptLang}`,
    "",
    `Write surrounding natural-language prose in **${promptLang}**.`,
    `All generated prose, including prose titles and section headings, must be in ${promptLang}.`,
    `Do not translate, transliterate, or describe proper nouns and technical identifiers unless the source already uses a well-established localized form.`,
    `Preserve organization names, product names, model names, dataset names, tool/library names, acronyms, code identifiers, file names, URLs, paper titles, citation strings, and technical terms that have no widely-used localized equivalent in their standard original form.`,
    `The source material or wiki content may be in a different language; use it as evidence, but keep generated prose in ${promptLang}.`,
    `This language rule overrides weaker style instructions, but it does not override the proper-noun and technical-identifier preservation rule above.`,
  ].join("\n")
}

/**
 * Short reminder version — for placing right before user's current message.
 */
export function buildLanguageReminder(fallbackText: string = ""): string {
  const lang = getOutputLanguage(fallbackText)
  return `REMINDER: Write prose in ${getLanguagePromptName(lang)}; preserve names, acronyms, identifiers, URLs, file names, and paper titles in their standard original form.`
}
