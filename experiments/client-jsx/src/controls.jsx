// The copied dioxus-components controls' markup (`components/`), so the shared style sheet draws
// them as it draws the Dioxus client's: a button, a text field and a select's closed trigger. A
// select's popup is the Dioxus client's overlay module, not part of this experiment.

export function Button({ variant = "outline", size = "default", children, ...rest }) {
  return (
    <button type="button" className="dx-button" data-style={variant} data-size={size} {...rest}>
      {children}
    </button>
  );
}

export function Input(props) {
  return <input className="dx-input" {...props} />;
}

export function Textarea({ variant = "default", ...rest }) {
  return <textarea className="dx-textarea" data-slot="textarea" data-style={variant} {...rest} />;
}

export function SelectTrigger({ id, label, value, options, disabled = false }) {
  const text = options.find(([key]) => key === value)?.[1] ?? "";
  return (
    <div className="dx-select">
      <button id={id} type="button" role="combobox" className="dx-select-trigger" disabled={disabled} aria-label={label} aria-haspopup="listbox" aria-expanded="false" aria-controls={id ? `${id}-popup` : undefined}>
        <span>{text}</span>
        <svg aria-hidden="true" width={20} height={20} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
    </div>
  );
}
