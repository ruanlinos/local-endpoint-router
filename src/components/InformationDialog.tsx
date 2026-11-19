import { Info } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { openExternal } from "@/lib/external";

const LINKS = [
  { label: "LinkedIn", href: "https://www.linkedin.com/in/linosruan", text: "linkedin.com/in/linosruan" },
  { label: "GitHub", href: "https://github.com/ruanlinos", text: "github.com/ruanlinos" },
];

export function InformationDialog() {
  return (
    <Dialog>
      <DialogTrigger
        type="button"
        className="flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-sm text-muted-foreground transition-colors hover:bg-sidebar-accent/70 hover:text-foreground"
      >
        <Info className="size-4" />
        Information
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Information</DialogTitle>
          <p className="mt-3 text-sm font-medium text-foreground">Ruan Linos Alves</p>
          <DialogDescription>Author of Local Endpoint Router.</DialogDescription>
        </DialogHeader>
        <dl className="divide-y divide-border border-y border-border text-sm">
          {LINKS.map((link) => (
            <div key={link.label} className="flex items-baseline justify-between gap-4 py-2">
              <dt className="text-muted-foreground">{link.label}</dt>
              <dd>
                <a
                  href={link.href}
                  className="text-primary hover:underline"
                  onClick={(event) => {
                    event.preventDefault();
                    openExternal(link.href);
                  }}
                >
                  {link.text}
                </a>
              </dd>
            </div>
          ))}
        </dl>
      </DialogContent>
    </Dialog>
  );
}
