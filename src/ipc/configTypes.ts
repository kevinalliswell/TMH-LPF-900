export interface SerialConfig {
  port: string;
  baudrate: number;
  bytesize: number;
  parity: string;
  stopbits: number;
  timeout: number;
}
export type GasKey = "H2" | "N2" | "CO2" | "CO";
export interface CommConfig {
  COM_RS485_TEMP: SerialConfig & {
    slave_address: number;
    start_reg: number;
    reg_count: number;
    TEMP_CHANNELS: string[];
    scale: number;
    signed_registers: boolean;
  };
  COM_RS485_MFC: SerialConfig;
  COM_RS232_Balance: SerialConfig;
  SAMPLING: { interval_s: number };
  PERFORMANCE_CONFIG: { data_collection_interval: number; experiment_data_interval: number; heartbeat_timeout: number };
  SLAVE_ADDRESS_MFC: Record<GasKey, number>;
  FLOW_SCALING: Record<GasKey, number>;
  GAS_SAFETY_LIMITS: { H2: number; CO: number };
}
export interface ConfigSnapshot {
  path: string;
  revision: string;
  applied: CommConfig;
  saved: CommConfig;
  restart_required: boolean;
}
