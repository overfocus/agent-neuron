export type NeuronSpec = {
    id: string;
    description?: string;
    at?: [number, number, number];
    source?: true;
    run?: string[];
    serve?: string[];
    http?: string;
    human?: true;
    network?: string;
    neuron?: string;
    threshold?: number;
    within?: number;
    maxHops?: number;
    timeout?: number;
    env?: string[];
    cwd?: string;
    cost?: Record<string, number>;
    effect?: true;
    may?: MaySpec;
    maxInflight?: number;
    each?: true;
};
/** What a neuron's program is allowed to touch (SPEC 2.4, decision 0016). A member that is absent grants nothing. */
export type MaySpec = {
    connect?: string[];
    listen?: string[];
    read?: string[];
    write?: string[];
};
/** The ground (SPEC 4.5, decision 0012): deposits per place, evaporation, and one-link routing for replies that name no `to`. */
export type GroundSpec = {
    deposit?: {
        on: string;
        add: string;
        by?: string;
    };
    evaporate?: number;
    select?: true;
    base?: number;
    power?: number;
    seed?: number;
};
export type NetworkFile = {
    version: 0;
    name?: string;
    description?: string;
    entry?: string;
    exit?: string;
    neurons: NeuronSpec[];
    links?: {
        from: string;
        to: string;
    }[];
    budget?: Record<string, number>;
    ground?: GroundSpec;
};
export type Finding = {
    severity: 'error' | 'warning';
    file: string;
    line?: number;
    column?: number;
    message: string;
};
type Schema = {
    [key: string]: unknown;
};
type Problem = {
    path: string;
    message: string;
};
/** Problems with `value` against `node` of the schema; each has a JSON path such as `$.neurons[1].filter`. */
export declare function validate(value: unknown, node?: Schema, path?: string, root?: Schema): Problem[];
export declare class BadSettings extends Error {
}
/**
 * A neuron's own settings (SPEC 2.4, decision 0022 point 6): `.env` in the directory of the file that declares its program
 * (`dir` for a neuron written inline), as the values of the names in its `env` that the file has. Lines are `NAME=value`;
 * blank lines and `#` lines are skipped, a quoted value loses its quotes, and anything else throws, naming the line and never its text.
 */
export declare function settingsOf(spec: NeuronSpec, dir: string): Record<string, string>;
/**
 * Reads and parses a network file; throws if it is not JSON. Does not validate. Every entry that names a neuron's own
 * file is replaced by the neuron it describes (SPEC 2.10), so nothing after this sees the `neuron` kind.
 */
export declare function readNetwork(file: string): NetworkFile;
/**
 * Everything `check` must report (SPEC 2.7); composed files are checked too, relative to `file`. Errors make the file unusable.
 * `shown` is only for composed files: their path relative to the outer file's directory, which findings name them by.
 */
export declare function check(file: string, seen?: string[], shown?: string): Finding[];
export declare const formatFinding: (f: Finding) => string;
export {};
