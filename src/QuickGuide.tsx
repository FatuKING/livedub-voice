import Card from "./components/Card";
import { useT } from "./i18n/context";
import type { MessageKey } from "./i18n/types";
import type { Direction } from "./lib/languages";
import "./QuickGuide.css";

/** Apps in the microphone table, in order. */
const MIC_APPS = ["discord", "meet", "teams", "whatsapp", "games"] as const;

function DirectionSteps({ direction }: { direction: Direction }) {
  const t = useT();
  const steps: MessageKey[] = [
    `guide.${direction}.step1`,
    `guide.${direction}.step2`,
    `guide.${direction}.step3`,
  ];
  return (
    <Card
      direction={direction}
      eyebrow={t(`guide.${direction}.eyebrow`)}
      title={t(`guide.${direction}.title`)}
    >
      <ol className="ld-guide__steps">
        {steps.map((key) => (
          <li key={key}>{t(key)}</li>
        ))}
      </ol>
    </Card>
  );
}

/**
 * How to use LiveDub Voice: how to start each direction, and which microphone to pick in the
 * usual call apps and games. Shown as the onboarding's last step and from Home ("How to use").
 */
function QuickGuide() {
  const t = useT();
  return (
    <div className="ld-guide">
      <div className="ld-route-grid ld-route-grid--stretch">
        <DirectionSteps direction="incoming" />
        <DirectionSteps direction="outgoing" />
      </div>
      <Card eyebrow={t("guide.mic.eyebrow")} title={t("guide.mic.title")}>
        <p className="ld-guide__intro">{t("guide.mic.intro")}</p>
        <table className="ld-guide__table">
          <caption className="visually-hidden">
            {t("guide.mic.caption")}
          </caption>
          <thead>
            <tr>
              <th scope="col">{t("guide.mic.app")}</th>
              <th scope="col">{t("guide.mic.where")}</th>
            </tr>
          </thead>
          <tbody>
            {MIC_APPS.map((app) => (
              <tr key={app}>
                <th scope="row">{t(`guide.mic.${app}`)}</th>
                <td>{t(`guide.mic.${app}Where`)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

export default QuickGuide;
