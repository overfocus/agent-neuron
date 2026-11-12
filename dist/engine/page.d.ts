import type { Desk, Question } from './engine.js';
/** One question on the page, and what became of it. */
export type Item = Question & {
    state: 'waiting' | 'answered' | 'withdrawn';
    kind?: 'approve' | 'edit' | 'no';
    answer?: string;
};
/**
 * What Approve sends (decision 0008, SPEC Appendix D): a payload's `proposal` when it is text, the payload itself when it is text,
 * and otherwise the payload as JSON text.
 */
export declare function approval(payload: unknown): string;
/** Starts the page. Resolves the desk to give `runNetwork`, the address to show the person, and `close`. */
export declare function openPage(options?: {
    title?: string;
    port?: number;
    token?: string;
}): Promise<{
    desk: Desk;
    url: string;
    token: string;
    items: () => Item[];
    close: () => Promise<void>;
}>;
