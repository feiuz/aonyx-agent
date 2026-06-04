export default function EmptyState({ icon: Icon, title, note }) {
  return (
    <div className="flex-1 flex items-center justify-center p-8">
      <div className="text-center text-aonyx-500 max-w-sm">
        {Icon && <Icon className="w-12 h-12 mx-auto opacity-25" strokeWidth={1.5} />}
        {title && (
          <h2 className="mt-3 font-cond uppercase tracking-wider text-aonyx-700 dark:text-aonyx-200">{title}</h2>
        )}
        {note && <p className="mt-1 text-sm">{note}</p>}
      </div>
    </div>
  );
}
