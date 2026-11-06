/** A question the page asks before a destructive action */
export type Question = {
  title: string;
  body: string;
  confirm: string;
  /** the other button's words, Cancel by default, for a question whose confirm cancels */
  dismiss?: string;
  run: () => void;
};

/** The page's one shared confirm dialog: what it asks, while it is open */
class Confirmation {
  question = $state.raw<Question | null>(null);
  open = $state(false);

  /** Asks `question`; its `run` runs only if the user confirms */
  ask(question: Question) {
    this.question = question;
    this.open = true;
  }
}

export const confirmation = new Confirmation();
