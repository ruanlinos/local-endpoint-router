import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";
import { DashboardPage } from "@/features/dashboard/DashboardPage";
import { MappingsPage } from "@/features/mappings/MappingsPage";
import { ProfilesPage } from "@/features/profiles/ProfilesPage";
import { ServicesPage } from "@/features/services/ServicesPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { TrafficPage } from "@/features/traffic/TrafficPage";
import { AppShell } from "@/layouts/AppShell";

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route element={<AppShell />}>
          <Route path="/" element={<DashboardPage />} />
          <Route path="/mappings" element={<MappingsPage />} />
          <Route path="/traffic" element={<TrafficPage />} />
          <Route path="/services" element={<ServicesPage />} />
          <Route path="/profiles" element={<ProfilesPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
