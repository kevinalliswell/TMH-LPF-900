/** Wire DTOs generated from the Rust config schema. */
export type {
  ConfigSnapshot,
  CommConfig,
  SerialParams as SerialConfig,
} from "./types.generated";

/** Frontend key used to index the four configured gas channels. */
export type GasKey = "H2" | "N2" | "CO2" | "CO";
