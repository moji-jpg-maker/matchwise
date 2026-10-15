'use client';

import React from 'react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';

interface MainContentProps {
  children: React.ReactNode;
}

const MainContent: React.FC<MainContentProps> = ({ children }) => {
  const { isCollapsed } = useSidebar();

  return (
    <main
      className={`flex-1 min-w-0 min-h-0 overflow-y-auto overflow-x-hidden custom-scrollbar transition-all duration-300 ${
        isCollapsed ? 'ms-16' : 'ms-64'
      }`}
    >
      <div className="ps-8 min-w-0 w-full max-w-full">
        {children}
      </div>
    </main>
  );
};

export default MainContent;
