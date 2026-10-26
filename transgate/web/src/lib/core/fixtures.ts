// Named view models produced by the Rust core (`presentation::fixtures::export_json`,
// written next to the generated types). Stories and component tests render
// these and nothing hand-written, so they show exactly what core hands a view.
import { fixtures as data, type Fixtures } from "./fixtures.generated";

/** One named fixture of a view-model type; throws for an unknown name so a stale story fails loudly. */
export function fixture<K extends keyof Fixtures>(type: K, name: string): Fixtures[K] {
  const view = data[type]?.[name];
  if (view === undefined) throw new Error(`no ${type} fixture named ${name}; regenerate core fixtures`);
  return view;
}

/** Every fixture name of a type, for stories that list all states. */
export function fixtureNames<K extends keyof Fixtures>(type: K): string[] {
  return Object.keys(data[type] ?? {});
}
