/** A reactive box for test data a mounted component reads, so a test can change it */
export const box = <T>(value: T): { value: T } => {
  const held = $state({ value });
  return held;
};
