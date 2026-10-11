/* TokenSonar · 首次启动语言引导页（React 版）
 *
 * 由旧 ui/assets/js/onboarding.js 平移：应用没有保存过语言时（settings.language 为空），
 * 强制展示本引导页；用户点选语言后界面文案立即切换预览，点「开始使用」才写入设置并进入应用。
 */
import { useEffect, useState } from "react";
import { BrandMark, Icon } from "@/lib/icons";
import { useT, setLocale, detect, available } from "@/lib/i18n";

export function Onboarding(props: { onDone: (lang: string) => void }) {
  const tr = useT();
  // 默认按系统语言推断，用户可再切换
  const [preview, setPreview] = useState<string>(detect);

  // 首帧按推断出的语言预览界面文案
  useEffect(() => {
    setLocale(preview);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const choose = (code: string) => {
    if (code === preview) return;
    // 切换预览语言会触发全局重绘（含本引导页）
    setLocale(code);
    setPreview(code);
  };

  const start = () => {
    // 点「开始使用」才落定语言；写入设置交由上层 onDone 处理
    setLocale(preview);
    props.onDone(preview);
  };

  return (
    <div className="onboarding-root is-open">
      <div className="ob-glow ob-glow-a" />
      <div className="ob-glow ob-glow-b" />
      <div className="ob-grid" />
      <div className="ob-card">
        <div className="ob-mark">
          <BrandMark />
        </div>
        <div className="ob-wordmark">TokenSonar</div>
        <div className="ob-tagline">{tr("onboarding.tagline")}</div>
        <div className="ob-sep" />
        <h1 className="ob-title">{tr("onboarding.title")}</h1>
        <p className="ob-sub">{tr("onboarding.subtitle")}</p>
        <div className="ob-options">
          {available.map((o) => {
            const isZh = o.code === "zh";
            return (
              <button
                key={o.code}
                type="button"
                className={"ob-option" + (preview === o.code ? " is-active" : "")}
                data-lang={o.code}
                onClick={() => choose(o.code)}
              >
                <span className={"ob-glyph" + (isZh ? "" : " is-en")}>{isZh ? "中" : "EN"}</span>
                <span className="ob-option-text">
                  <b>{tr("onboarding." + o.code + "Name")}</b>
                  <i>{tr("onboarding." + o.code + "Desc")}</i>
                </span>
                <span className="ob-check">
                  <Icon name="check" />
                </span>
              </button>
            );
          })}
        </div>
        <button type="button" className="btn btn-primary ob-start" id="obStart" onClick={start}>
          {tr("onboarding.start")}
        </button>
      </div>
    </div>
  );
}