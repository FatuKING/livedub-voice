import { ArrowLeft, ArrowRight, Check } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import Button from "../components/Button";
import LogoMark from "../components/LogoMark";
import PageHeader from "../components/PageHeader";
import Stepper from "../components/Stepper";
import { useT } from "../i18n/context";
import type { MessageKey } from "../i18n/types";
import { completeOnboarding } from "../lib/api";
import {
  ONBOARDING_STEPS,
  type OnboardingMode,
  type OnboardingStep,
} from "../lib/onboarding";
import CableStep from "./CableStep";
import {
  ApiKeyStep,
  GuideStep,
  HeadphonesStep,
  PrivacyStep,
  WelcomeStep,
} from "./steps";
import "./Onboarding.css";

const HEADINGS: Record<
  OnboardingStep,
  { title: MessageKey; description: MessageKey }
> = {
  welcome: {
    title: "onboarding.welcome.title",
    description: "onboarding.welcome.description",
  },
  apiKey: {
    title: "onboarding.apiKey.title",
    description: "onboarding.apiKey.description",
  },
  headphones: {
    title: "onboarding.headphones.title",
    description: "onboarding.headphones.description",
  },
  cable: {
    title: "onboarding.cable.title",
    description: "onboarding.cable.description",
  },
  privacy: {
    title: "onboarding.privacy.title",
    description: "onboarding.privacy.description",
  },
  guide: { title: "guide.title", description: "guide.description" },
};

export interface OnboardingProps {
  mode: OnboardingMode;
  /** Called once the setup is finished or skipped (and remembered, when possible). */
  onDone: () => void;
}

/**
 * First-run setup: short steps, each skippable, with a step indicator. Finishing or skipping it
 * marks it as done in the settings, so it does not show again (Settings can run it again).
 */
function Onboarding({ mode, onDone }: OnboardingProps) {
  const t = useT();
  const steps = ONBOARDING_STEPS[mode];
  const [index, setIndex] = useState(0);
  const [finishing, setFinishing] = useState(false);
  const contentRef = useRef<HTMLElement>(null);
  const moved = useRef(false);
  const step = steps[index];
  const last = index === steps.length - 1;

  // After moving between steps, focus the new step's title so it is announced.
  useEffect(() => {
    if (!moved.current) return;
    contentRef.current?.scrollTo?.({ top: 0 });
    contentRef.current?.querySelector<HTMLElement>("h1")?.focus();
  }, [index]);

  function go(next: number) {
    moved.current = true;
    setIndex(next);
  }

  async function finish() {
    setFinishing(true);
    // If it cannot be saved the setup shows again next time: never keep the user here for it.
    await completeOnboarding().catch(() => undefined);
    onDone();
  }

  const heading = HEADINGS[step];

  return (
    <div className="ld-shell">
      <aside className="ld-sidebar">
        <div className="ld-brand">
          <LogoMark />
          <span className="ld-brand__name">LiveDub Voice</span>
        </div>
        <Stepper
          label={t("onboarding.stepsLabel")}
          doneLabel={t("onboarding.done")}
          steps={steps.map((id) => ({
            id,
            label: t(`onboarding.steps.${id}`),
          }))}
          current={index}
          onSelect={go}
        />
        <footer className="ld-sidebar__footer ld-onboarding__skip">
          <Button
            variant="ghost"
            disabled={finishing}
            onClick={() => void finish()}
          >
            {t("onboarding.skip")}
          </Button>
          <p>{t("onboarding.skipHint")}</p>
        </footer>
      </aside>

      <main className="ld-content" ref={contentRef}>
        <div className="ld-content__inner">
          <PageHeader
            eyebrow={t("onboarding.stepOf", {
              step: index + 1,
              total: steps.length,
            })}
            title={t(heading.title)}
            description={t(heading.description)}
          />
          {step === "welcome" && <WelcomeStep />}
          {step === "apiKey" && <ApiKeyStep />}
          {step === "headphones" && <HeadphonesStep />}
          {step === "cable" && <CableStep />}
          {step === "privacy" && <PrivacyStep light={mode === "light"} />}
          {step === "guide" && <GuideStep />}
          <div className="ld-onboarding__nav">
            {index > 0 && (
              <Button
                variant="secondary"
                icon={<ArrowLeft size={16} aria-hidden="true" />}
                onClick={() => go(index - 1)}
              >
                {t("onboarding.back")}
              </Button>
            )}
            {last ? (
              <Button
                className="ld-onboarding__next"
                disabled={finishing}
                icon={<Check size={16} aria-hidden="true" />}
                onClick={() => void finish()}
              >
                {t("onboarding.finish")}
              </Button>
            ) : (
              <Button
                className="ld-onboarding__next"
                icon={<ArrowRight size={16} aria-hidden="true" />}
                onClick={() => go(index + 1)}
              >
                {t("onboarding.next")}
              </Button>
            )}
          </div>
        </div>
      </main>
    </div>
  );
}

export default Onboarding;
