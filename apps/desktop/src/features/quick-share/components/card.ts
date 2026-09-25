/** A share card. It dims while it's being stopped (`aria-busy`), so it's clearly on its way out. */
export const cardClass =
  "flex flex-col gap-3 rounded-card bg-surface-inset p-4 transition-opacity transition-smooth aria-busy:opacity-60";
