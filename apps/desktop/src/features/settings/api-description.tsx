import { toast } from "sonner";
import { GroupedRow, GroupedSection } from "@/components/patterns/grouped-list";
import { Button } from "@/components/ui/button";
import { t } from "@/lib/i18n";
import { toIpcError } from "@/lib/ipc/client";
import { useSaveOpenApi } from "./queries";

/**
 * Settings ▸ General ▸ Captured Requests: an OpenAPI description of the API the
 * inspector's captured requests show, saved to Downloads (the inspector's own screens
 * can offer it per share later).
 */
export function ApiDescription() {
  const save = useSaveOpenApi();
  return (
    <GroupedSection title={t("settings.openapi.title")} footer={t("settings.openapi.footer")}>
      <GroupedRow label={t("settings.openapi.label")} description={t("settings.openapi.detail")}>
        <Button
          size="sm"
          pending={save.isPending}
          onClick={() =>
            save.mutate(undefined, {
              onSuccess: ({ summary }) =>
                summary.requests === 0
                  ? toast(t("settings.openapi.empty"))
                  : toast.success(
                      t("settings.openapi.saved", {
                        paths: summary.paths,
                        count: summary.requests,
                      }),
                    ),
              onError: (error) => toast.error(toIpcError(error).message),
            })
          }
        >
          {t("settings.openapi.save")}
        </Button>
      </GroupedRow>
    </GroupedSection>
  );
}
