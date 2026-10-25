import { recentPrograms, useAllPrograms } from "../api/allPrograms";
import { ProgramTile } from "./ProgramsTab";
import type { Route } from "./Sidebar";

/** How many programs "Recently used" shows. */
const COUNT = 6;

/** Home page row: the programs opened most recently, across bottles. */
export function RecentApps({ onNavigate }: { onNavigate: (r: Route) => void }) {
  const apps = useAllPrograms();
  const recent = recentPrograms(apps, COUNT);
  // With only a few apps, the row would just repeat the list below.
  if (recent.length === 0 || apps.length <= COUNT) return null;
  return (
    <>
      <section className="flex flex-col gap-2" aria-label="Recently used">
        <h2 className="text-sm font-medium">Recently used</h2>
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-3">
          {recent.map(({ bottle, program, needsRosetta }) => {
            const open = () => onNavigate({ page: "bottle", id: bottle.id });
            return (
              <ProgramTile
                key={`${bottle.id}-${program.index}-${program.exe}`}
                bottleId={bottle.id}
                program={program}
                needsRosetta={needsRosetta}
                onShowLog={open}
                bottle={{ name: bottle.name, open }}
              />
            );
          })}
        </ul>
      </section>
      {/* Titles the full list that follows. */}
      <h2 className="-mb-4 text-sm font-medium">All apps</h2>
    </>
  );
}
