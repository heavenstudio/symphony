// A control for the probe, not a fourth library: Radix's range slider, which Preact's own tracker
// has held as broken under compat since 2022 (its thumbs are not drawn). Nothing the client draws
// is a slider; it is here to show that the checks can tell a library that does not work.
import { useState } from "react";
import { Slider } from "radix-ui";

export default function RangeSlider() {
  const [range, setRange] = useState([20, 80]);
  return (
    <section id="slider" data-ready="true">
      <Slider.Root id="range" value={range} onValueChange={setRange} min={0} max={100} step={1} style={{ position: "relative", display: "flex", alignItems: "center", width: 300, height: 20 }}>
        <Slider.Track style={{ position: "relative", flexGrow: 1, height: 4, background: "#ccc" }}>
          <Slider.Range style={{ position: "absolute", height: "100%", background: "#333" }} />
        </Slider.Track>
        <Slider.Thumb aria-label="From" style={{ display: "block", width: 16, height: 16, borderRadius: 8, background: "#06c" }} />
        <Slider.Thumb aria-label="To" style={{ display: "block", width: 16, height: 16, borderRadius: 8, background: "#06c" }} />
      </Slider.Root>
      <output id="range-value">{range.join("-")}</output>
    </section>
  );
}
