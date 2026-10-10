export interface Question {
  title: string;
  text: string;
  /** The label of the button that agrees. */
  yes: string;
  danger?: boolean;
}

/** One modal question at a time; a new one answers the previous with «no». */
class Confirmation {
  question = $state<Question | null>(null);
  #resolve: ((yes: boolean) => void) | null = null;

  ask(question: Question): Promise<boolean> {
    this.#resolve?.(false);
    this.question = question;
    return new Promise((resolve) => (this.#resolve = resolve));
  }

  answer(yes: boolean): void {
    this.#resolve?.(yes);
    this.#resolve = null;
    this.question = null;
  }
}

export const confirmation = new Confirmation();
