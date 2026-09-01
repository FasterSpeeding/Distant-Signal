import type { MetadataRoute } from 'next';

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: 'Distant Signal',
    short_name: 'Distant Signal',
    description:
      'A personal UK rail companion: live line status, train tracking, and ticket/Delay-Repay support.',
    start_url: '/',
    display: 'standalone',
    background_color: '#ffffff',
    theme_color: '#be4bdb',
    icons: [
      { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
      { src: '/icon-512.png', sizes: '512x512', type: 'image/png' },
    ],
  };
}
