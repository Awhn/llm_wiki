import i18n from "i18next"
import { initReactI18next } from "react-i18next"
import en from "./en.json"
import ko from "./ko.json"
import it from "./it.json"
import zh from "./zh.json"
import ru from "./ru.json"

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    ko: { translation: ko },
    it: { translation: it },
    zh: { translation: zh },
    ru: { translation: ru },
  },
  lng: /^ko(?:-|$)/i.test(globalThis.navigator?.language ?? "") ? "ko" : "en",
  fallbackLng: "en",
  interpolation: { escapeValue: false },
})

export function closeDialogLabels(language: string) {
  const t = i18n.getFixedT(language)
  return { message: t("app.closeMessage"), quit: t("app.quit"), hide: t("app.hide") }
}

export default i18n
