// Whose a task is, the chunk a task's page imports for a task with an owner alone (task-page.tsx's
// `OwnerPart`, chunk.tsx): owner.tsx's line. A module of its own rather than an import of owner.tsx,
// which a document's page imports at once: the build draws the namespace of a module that a page
// imports at once and another later with a helper of its own, in a file every page then loads.

export { OwnerLine } from "./owner";
