export default function StatCard({ icon: Icon, label, value, small }) {
  return (
    <div className="rounded-xl border border-aonyx-200 dark:border-aonyx-800 p-4 bg-aonyx-100/50 dark:bg-aonyx-950/50">
      <div className="flex items-center gap-2 text-xs uppercase tracking-wide text-aonyx-500">
        {Icon && <Icon className="w-4 h-4" strokeWidth={1.75} />}
        {label}
      </div>
      <div
        className={`mt-2 font-cond text-aonyx-900 dark:text-aonyx-100 ${small ? "text-base break-all" : "text-3xl"}`}
      >
        {value}
      </div>
    </div>
  );
}
