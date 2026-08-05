/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        ink: {
          DEFAULT: '#090d16',
          900: '#0d1120',
          800: '#141b2f',
          700: '#1a2338',
          600: '#242f4a',
        },
        accent: {
          DEFAULT: '#6366f1',
          hover: '#818cf8',
          violet: '#8b5cf6',
          glow: 'rgba(99, 102, 241, 0.35)',
        },
        success: {
          DEFAULT: '#10b981',
          soft: 'rgba(16, 185, 129, 0.12)',
          border: 'rgba(16, 185, 129, 0.25)',
        },
        warning: {
          DEFAULT: '#f59e0b',
          soft: 'rgba(245, 158, 11, 0.12)',
          border: 'rgba(245, 158, 11, 0.25)',
        },
        danger: {
          DEFAULT: '#f43f5e',
          soft: 'rgba(244, 63, 94, 0.12)',
          border: 'rgba(244, 63, 94, 0.25)',
        },
        info: {
          DEFAULT: '#38bdf8',
          soft: 'rgba(56, 189, 248, 0.12)',
          border: 'rgba(56, 189, 248, 0.25)',
        },
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', '-apple-system', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'ui-monospace', 'SFMono-Regular', 'monospace'],
      },
      boxShadow: {
        glow: '0 0 24px rgba(99, 102, 241, 0.35)',
        'glow-sm': '0 0 12px rgba(99, 102, 241, 0.30)',
        'glow-emerald': '0 0 12px rgba(16, 185, 129, 0.30)',
        card: 'inset 0 1px 0 rgba(255,255,255,0.05), 0 8px 24px rgba(0,0,0,0.3)',
      },
      backgroundImage: {
        'grid-faint':
          'radial-gradient(circle at 12% 18%, rgba(99,102,241,0.12), transparent 42%), radial-gradient(circle at 88% 82%, rgba(16,185,129,0.08), transparent 42%)',
        'brand-gradient': 'linear-gradient(135deg, #6366f1, #8b5cf6)',
      },
      animation: {
        'fade-in': 'fadeIn 0.25s ease-out',
        'slide-up': 'slideUp 0.3s ease-out',
        'pulse-soft': 'pulseSoft 2.4s ease-in-out infinite',
      },
      keyframes: {
        fadeIn: {
          '0%': { opacity: '0' },
          '100%': { opacity: '1' },
        },
        slideUp: {
          '0%': { opacity: '0', transform: 'translateY(8px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' },
        },
        pulseSoft: {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0.45' },
        },
      },
    },
  },
  plugins: [],
};