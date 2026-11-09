import { ChartColumn } from "lucide-react";
import Card from "../components/Card";
import PageHeader from "../components/PageHeader";
import "./screens.css";

/** Usage placeholder until local metering exists. */
function UsageScreen() {
  return (
    <>
      <PageHeader
        title="Usage"
        description="Minutes translated and estimated cost, per session and over time."
      />
      <Card className="ld-empty">
        <span className="ld-empty__icon" aria-hidden="true">
          <ChartColumn size={24} />
        </span>
        <h2 className="ld-empty__title">No usage yet</h2>
        <p className="ld-empty__text">
          Once you start translating, the minutes for each direction and their
          estimated cost show up here. They are counted on this computer at{" "}
          <span className="mono">$0.034</span> per audio minute; LiveDub Voice
          does not read your OpenAI billing.
        </p>
      </Card>
    </>
  );
}

export default UsageScreen;
