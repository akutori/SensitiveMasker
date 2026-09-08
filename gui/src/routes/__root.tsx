import { Outlet, createRootRoute } from "@tanstack/react-router";
import { TanStackRouterDevtools } from "@tanstack/react-router-devtools";
import { InitialSetupScreen } from "@/components/initial-setup-screen";
import { useAppState } from "@/lib/app-state";

export const Route = createRootRoute({
  component: RootComponent,
});

function RootComponent() {
  const { initialized, start } = useAppState();

  return (
    <>
      {initialized ? <Outlet /> : <InitialSetupScreen onStart={start} />}
      <TanStackRouterDevtools position="bottom-right" />
    </>
  );
}
