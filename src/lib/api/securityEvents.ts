import { invoke } from "@tauri-apps/api/core";
import type {
  SecurityEventFilter,
  SecurityEventPage,
  SecurityEventSummary,
  SecurityEventsClearReceipt,
} from "@/types/securityEvents";

export const securityEventsApi = {
  async getEvents(
    filter: SecurityEventFilter,
    page: number,
    pageSize: number,
  ): Promise<SecurityEventPage> {
    return invoke("get_security_events", { filter, page, pageSize });
  },

  async getSummary(filter: SecurityEventFilter): Promise<SecurityEventSummary> {
    return invoke("get_security_events_summary", { filter });
  },

  async clearEvents(
    filter: SecurityEventFilter,
  ): Promise<SecurityEventsClearReceipt> {
    return invoke("clear_security_events", { filter });
  },
};
