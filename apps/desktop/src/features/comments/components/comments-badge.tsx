import { t } from "@/lib/i18n";
import { useCommentSubjects } from "../queries";

/** The sidebar's count of comments reviewers wrote since the owner last looked. */
export function CommentsBadge() {
  const { data } = useCommentSubjects();
  const unread = (data ?? []).reduce((sum, subject) => sum + subject.unread, 0);
  if (unread === 0) return null;
  return (
    <span
      role="status"
      aria-label={t("comments.unread", { count: unread })}
      className="flex h-4 min-w-4 items-center justify-center rounded-full bg-accent-fill px-1 text-footnote font-semibold text-on-accent tabular group-data-[status=active]:bg-on-accent group-data-[status=active]:text-accent"
    >
      {unread}
    </span>
  );
}
