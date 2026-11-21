export function WorkspacePageHeading({
  title,
  description,
}: {
  title: string;
  description?: string;
}) {
  return (
    <div className="workspace-page-heading">
      <h1>{title}</h1>
      {description && <p>{description}</p>}
    </div>
  );
}
